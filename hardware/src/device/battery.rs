//! The battery: the pack the EC's block answers for, the charger that shapes
//! what goes into it, and the mirror for the one limit that cannot be read
//! back.

use std::num::NonZeroU32;
use std::sync::Arc;

use frameguin_wire::{
    BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, DeviceError, DeviceResult,
    MIN_CHARGE_LIMIT, NO_CHARGE_CURRENT_LIMIT,
};

use crate::ec::{Charger, Ec, Pack};
use crate::lifetime::Lifetime;
use crate::mirror::{Mirror, Mirrors};
use crate::part::{Identity, Part};

const KEY_CURRENT_LIMIT: &str = "charge_current_limit";

pub struct Battery {
    pack: Arc<dyn Pack>,
    charger: Arc<dyn Charger>,
    identity: Identity,
    features: Vec<BatteryFeature>,
    /// The cap last written, and nothing while there is none; the EC keeps
    /// it in RAM.
    current_limit: Mirror<NonZeroU32>,
}

impl Battery {
    /// The pack in the EC's block, if one answers there.
    pub fn detect(ec: &Arc<Ec>, mirrors: &Mirrors) -> Option<Self> {
        let identity = ec.identity()?;
        Some(Self::new(ec.clone(), ec.clone(), mirrors, identity))
    }

    /// What this battery offers is settled here, once. The condition is
    /// probed by the getter's own read — the only probe here that reaches
    /// the pack rather than the EC, and what it answers for is the I2C
    /// passthrough working, which nothing about a readable block promises.
    /// The current cap needs the command and the pack both: a limit is only
    /// ever expressed as a share of what the pack asks for, and the pack is
    /// what this device's presence already vouches for.
    pub fn new(
        pack: Arc<dyn Pack>,
        charger: Arc<dyn Charger>,
        mirrors: &Mirrors,
        identity: Identity,
    ) -> Self {
        let mut features = Vec::new();
        if pack.condition().is_some() {
            features.push(BatteryFeature::Condition);
        }
        if charger.charge_limit().is_ok() {
            features.push(BatteryFeature::ChargeLimit);
        }
        if charger.charge_current_limit_supported() {
            features.push(BatteryFeature::ChargeCurrentLimit);
        }
        Self {
            pack,
            charger,
            identity,
            features,
            current_limit: mirrors.value(KEY_CURRENT_LIMIT, Lifetime::Ec),
        }
    }

    /// Separate from the setter so a server can refuse an argument before it
    /// prompts for authorization.
    pub fn check_charge_limit(percent: u8) -> DeviceResult<()> {
        if (MIN_CHARGE_LIMIT..=100).contains(&percent) {
            Ok(())
        } else {
            Err(DeviceError::InvalidArgs(format!(
                "charge limit must be {MIN_CHARGE_LIMIT}-100"
            )))
        }
    }

    /// Zero is refused: the EC clamps its requested current against this
    /// value, so zero stops charging altogether rather than meaning
    /// "unrestricted", and nothing would report that back.
    pub fn check_charge_current_limit(milliamps: u32) -> DeviceResult<()> {
        if milliamps == 0 {
            Err(DeviceError::InvalidArgs(format!(
                "0 stops charging; pass {NO_CHARGE_CURRENT_LIMIT} to remove the limit"
            )))
        } else {
            Ok(())
        }
    }
}

impl Part for Battery {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl BatteryControl for Battery {
    /// Spelled apart from the condition below, which fails for a different
    /// reason and says so: a passthrough that stays silent is not an absent
    /// pack, and can happen with one fitted.
    async fn info(&self) -> DeviceResult<BatteryInfo> {
        self.pack
            .info()
            .ok_or_else(|| DeviceError::Failed("no battery present".into()))
    }

    async fn condition(&self) -> DeviceResult<BatteryCondition> {
        self.pack
            .condition()
            .ok_or_else(|| DeviceError::Failed("the battery did not answer".into()))
    }

    async fn features(&self) -> DeviceResult<Vec<BatteryFeature>> {
        Ok(self.features.clone())
    }

    async fn charge_limit(&self) -> DeviceResult<u8> {
        self.charger.charge_limit()
    }

    async fn set_charge_limit(&self, percent: u8) -> DeviceResult<bool> {
        Self::check_charge_limit(percent)?;
        self.charger.set_charge_limit(percent)?;
        Ok(true)
    }

    async fn charge_current_limit(&self) -> DeviceResult<u32> {
        Ok(self
            .current_limit
            .current()
            .map_or(NO_CHARGE_CURRENT_LIMIT, NonZeroU32::get))
    }

    async fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<bool> {
        Self::check_charge_current_limit(milliamps)?;
        let write = || self.charger.set_charge_current_limit(milliamps);
        match NonZeroU32::new(milliamps).filter(|cap| cap.get() != NO_CHARGE_CURRENT_LIMIT) {
            Some(cap) => self.current_limit.record(cap, write)?,
            None => self.current_limit.clear(write)?,
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_wire::{BatteryControl, BatteryFeature, DeviceError, NO_CHARGE_CURRENT_LIMIT};

    use super::{Battery, KEY_CURRENT_LIMIT};
    use crate::ec::Pack;
    use crate::lifetime::EcBoot;
    use crate::mirror::evidence_key;
    use crate::state::Store;
    use crate::testing::{EC_BOOT, EC_RESTARTED, EcCharger, Gauge, Memory, block, mirrors, ready};

    struct Machine {
        condition: bool,
        caps: bool,
        refusing: bool,
        ec_boot: Option<EcBoot>,
    }

    const FULL: Machine = Machine {
        condition: true,
        caps: true,
        refusing: false,
        ec_boot: Some(EC_BOOT),
    };

    const RESTARTED: Machine = Machine {
        ec_boot: Some(EC_RESTARTED),
        ..FULL
    };

    struct Bench {
        battery: Battery,
        ec: Arc<EcCharger>,
    }

    fn over(machine: &Machine, store: &Arc<Memory>) -> Bench {
        let pack = Arc::new(Gauge {
            answering: machine.condition,
        });
        let ec = Arc::new(EcCharger {
            caps: machine.caps,
            refusing: machine.refusing,
            ..EcCharger::default()
        });
        let identity = pack.identity().unwrap();
        let mirrors = mirrors(store, machine.ec_boot, None);
        Bench {
            battery: Battery::new(pack, ec.clone(), &mirrors, identity),
            ec,
        }
    }

    #[test]
    fn the_features_are_what_each_probe_answered() {
        let store = Arc::new(Memory::default());
        let full = over(&FULL, &store);
        assert_eq!(
            ready(full.battery.features()),
            Ok(vec![
                BatteryFeature::Condition,
                BatteryFeature::ChargeLimit,
                BatteryFeature::ChargeCurrentLimit
            ])
        );
        let bare = over(
            &Machine {
                condition: false,
                caps: false,
                ..FULL
            },
            &store,
        );
        assert_eq!(
            ready(bare.battery.features()),
            Ok(vec![BatteryFeature::ChargeLimit])
        );
        assert!(ready(bare.battery.condition()).is_err());
    }

    #[test]
    fn the_block_and_the_condition_come_from_the_pack() {
        let store = Arc::new(Memory::default());
        let Bench { battery, .. } = over(&FULL, &store);
        assert_eq!(ready(battery.info()), Ok(block()));
        assert_eq!(ready(battery.condition()).map(|c| c.decicelsius), Ok(301));
    }

    #[test]
    fn a_ceiling_is_written_and_read_from_the_charger() {
        let store = Arc::new(Memory::default());
        let Bench { battery, ec } = over(&FULL, &store);
        assert_eq!(ready(battery.set_charge_limit(80)), Ok(true));
        assert_eq!(*ec.limit.lock().unwrap(), 80);
        assert_eq!(ready(battery.charge_limit()), Ok(80));
    }

    #[test]
    fn a_ceiling_off_the_range_is_refused_before_the_charger_is_asked() {
        let store = Arc::new(Memory::default());
        let Bench { battery, ec } = over(&FULL, &store);
        for percent in [19, 101] {
            assert!(matches!(
                ready(battery.set_charge_limit(percent)),
                Err(DeviceError::InvalidArgs(_))
            ));
        }
        assert_eq!(*ec.limit.lock().unwrap(), 100);
    }

    #[test]
    fn an_empty_store_answers_no_current_limit() {
        let store = Arc::new(Memory::default());
        let Bench { battery, .. } = over(&FULL, &store);
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
    }

    #[test]
    fn a_cap_the_ec_takes_is_mirrored_stored_and_reloaded() {
        let store = Arc::new(Memory::default());
        let Bench { battery, ec } = over(&FULL, &store);
        assert_eq!(ready(battery.set_charge_current_limit(1_500)), Ok(true));
        assert_eq!(*ec.written.lock().unwrap(), [1_500]);
        assert_eq!(ready(battery.charge_current_limit()), Ok(1_500));
        assert_eq!(store.get(KEY_CURRENT_LIMIT).as_deref(), Some("1500"));
        assert!(store.get(&evidence_key(KEY_CURRENT_LIMIT)).is_some());
        let reloaded = over(&FULL, &store);
        assert_eq!(ready(reloaded.battery.charge_current_limit()), Ok(1_500));
    }

    #[test]
    fn a_cap_the_ec_refuses_leaves_the_mirror_standing() {
        let store = Arc::new(Memory::default());
        let Bench { battery, .. } = over(
            &Machine {
                refusing: true,
                ..FULL
            },
            &store,
        );
        assert!(ready(battery.set_charge_current_limit(1_500)).is_err());
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
        assert_eq!(store.get(KEY_CURRENT_LIMIT), None);
    }

    #[test]
    fn a_zero_cap_is_refused_before_the_ec_is_asked() {
        let store = Arc::new(Memory::default());
        let Bench { battery, ec } = over(&FULL, &store);
        assert!(matches!(
            ready(battery.set_charge_current_limit(0)),
            Err(DeviceError::InvalidArgs(_))
        ));
        assert!(ec.written.lock().unwrap().is_empty());
    }

    /// An EC that restarted has dropped the cap, whatever the mirror says.
    #[test]
    fn a_cap_from_another_ec_boot_reads_as_no_limit() {
        let store = Arc::new(Memory::default());
        ready(over(&FULL, &store).battery.set_charge_current_limit(1_500)).unwrap();
        let Bench { battery, .. } = over(&RESTARTED, &store);
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
    }

    /// A cap the EC will not witness is still written, and forgotten: with
    /// no evidence, nothing could say later whether it still stands.
    #[test]
    fn a_cap_with_no_evidence_is_written_and_not_remembered() {
        let store = Arc::new(Memory::default());
        let Bench { battery, ec } = over(
            &Machine {
                ec_boot: None,
                ..FULL
            },
            &store,
        );
        assert_eq!(ready(battery.set_charge_current_limit(1_500)), Ok(true));
        assert_eq!(*ec.written.lock().unwrap(), [1_500]);
        assert_eq!(store.get(KEY_CURRENT_LIMIT), None);
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
    }

    #[test]
    fn lifting_the_cap_clears_the_mirror_and_the_store() {
        let store = Arc::new(Memory::default());
        let Bench { battery, .. } = over(&FULL, &store);
        ready(battery.set_charge_current_limit(1_500)).unwrap();
        assert_eq!(
            ready(battery.set_charge_current_limit(NO_CHARGE_CURRENT_LIMIT)),
            Ok(true)
        );
        assert_eq!(store.get(KEY_CURRENT_LIMIT), None);
        assert_eq!(store.get(&evidence_key(KEY_CURRENT_LIMIT)), None);
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
    }

    #[test]
    fn a_stored_zero_reads_as_no_limit() {
        let store = Arc::new(Memory::default());
        store.set(KEY_CURRENT_LIMIT, Some("0".into()));
        store.set(&evidence_key(KEY_CURRENT_LIMIT), Some("500000".into()));
        let Bench { battery, .. } = over(&FULL, &store);
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
    }
}
