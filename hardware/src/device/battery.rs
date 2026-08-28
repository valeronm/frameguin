//! The battery: the pack the EC's block answers for, the charger that shapes
//! what goes into it, and the mirror for the one limit that cannot be read
//! back.

use std::sync::{Arc, Mutex};

use frameguin_wire::{
    BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, DeviceError, DeviceResult,
    MIN_CHARGE_LIMIT, NO_CHARGE_CURRENT_LIMIT,
};

use crate::ec::{Charger, Ec, EcClock, Pack};
use crate::lifetime::EcStamp;
use crate::part::{Identity, Part};
use crate::state::Store;

const KEY_CURRENT_LIMIT: &str = "charge_current_limit";
const KEY_CURRENT_LIMIT_STAMP: &str = "charge_current_limit_stamp";

/// A charge current limit together with the stamp that dates it.
#[derive(Clone, Copy)]
struct CurrentLimit {
    milliamps: u32,
    stamp: EcStamp,
}

pub struct Battery {
    pack: Arc<dyn Pack>,
    charger: Arc<dyn Charger>,
    clock: Arc<dyn EcClock>,
    store: Arc<dyn Store>,
    identity: Identity,
    features: Vec<BatteryFeature>,
    /// The cap last written, and None while there is none. Mirrored rather
    /// than read back, and dated against the EC: it keeps the limit in RAM,
    /// which outlives host reboots but not an EC restart.
    current_limit: Mutex<Option<CurrentLimit>>,
}

impl Battery {
    /// The pack in the EC's block, if one answers there.
    pub fn detect(ec: &Arc<Ec>, store: Arc<dyn Store>) -> Option<Self> {
        let identity = ec.identity()?;
        Some(Self::new(
            ec.clone(),
            ec.clone(),
            ec.clone(),
            store,
            identity,
        ))
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
        clock: Arc<dyn EcClock>,
        store: Arc<dyn Store>,
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
        // A zero here would mirror a limit the setter refuses to write, so
        // it reads as the absence of one — as does a cap with no stamp, which
        // nothing could weigh.
        let current_limit = store
            .get(KEY_CURRENT_LIMIT)
            .and_then(|v| v.parse().ok())
            .filter(|&v| v != 0)
            .zip(
                store
                    .get(KEY_CURRENT_LIMIT_STAMP)
                    .and_then(|v| EcStamp::parse(&v)),
            )
            .map(|(milliamps, stamp)| CurrentLimit { milliamps, stamp });
        Self {
            pack,
            charger,
            clock,
            store,
            identity,
            features,
            current_limit: Mutex::new(current_limit),
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

    fn remember(&self, limit: Option<CurrentLimit>) {
        self.store.set(
            KEY_CURRENT_LIMIT,
            limit.map(|limit| limit.milliamps.to_string()),
        );
        self.store.set(
            KEY_CURRENT_LIMIT_STAMP,
            limit.map(|limit| limit.stamp.stored()),
        );
        *self.current_limit.lock().unwrap() = limit;
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

    /// The mirror, weighed against the EC's life: a restart drops the cap.
    async fn charge_current_limit(&self) -> DeviceResult<u32> {
        let limit = *self.current_limit.lock().unwrap();
        Ok(limit
            .filter(|limit| self.clock.same_boot_as(limit.stamp))
            .map_or(NO_CHARGE_CURRENT_LIMIT, |limit| limit.milliamps))
    }

    async fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<bool> {
        Self::check_charge_current_limit(milliamps)?;
        // Dated before the write, so that a restart between the two reads as
        // having dropped it.
        let limit = (milliamps != NO_CHARGE_CURRENT_LIMIT)
            .then(|| self.clock.stamp().ok())
            .flatten()
            .map(|stamp| CurrentLimit { milliamps, stamp });
        self.charger.set_charge_current_limit(milliamps)?;
        self.remember(limit);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use frameguin_wire::{
        BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, BatteryState, ChargeFlow,
        DeviceError, DeviceResult, Identity, NO_CHARGE_CURRENT_LIMIT, PartKind,
    };

    use super::{Battery, KEY_CURRENT_LIMIT, KEY_CURRENT_LIMIT_STAMP};
    use crate::ec::{Charger, Pack};
    use crate::state::Store;
    use crate::state::tests::Memory;
    use crate::testing::{Clock, ready};

    fn block() -> BatteryInfo {
        BatteryInfo {
            state: BatteryState {
                percent: 80,
                flow: ChargeFlow::Idle,
                milliamps: 0,
                millivolts: 15_000,
            },
            remaining_capacity: 3_000,
            last_full_capacity: 3_600,
            design_capacity: 3_900,
            design_millivolts: 15_400,
            cycle_count: 12,
            charger_connected: true,
            critical: false,
            manufacturer: "NVT".into(),
            model: "FRANGWA".into(),
            serial: "0001".into(),
            chemistry: "LION".into(),
            manufactured: "2026-01-01".into(),
        }
    }

    /// A pack that answers its block, and its condition where told to.
    struct Gauge {
        answering: bool,
    }

    impl Pack for Gauge {
        fn identity(&self) -> Option<Identity> {
            Some(Identity {
                kind: PartKind::Battery,
                vendor: "NVT".into(),
                model: "FRANGWA".into(),
                serial: "0001".into(),
                id: "sbs:FRANGWA".into(),
                firmware: Vec::new(),
            })
        }

        fn info(&self) -> Option<BatteryInfo> {
            Some(block())
        }

        fn condition(&self) -> Option<BatteryCondition> {
            self.answering.then(|| BatteryCondition {
                cell_millivolts: vec![3_750, 3_751, 3_749, 3_750],
                alarms: Vec::new(),
                decicelsius: 301,
            })
        }
    }

    /// A charger holding one ceiling, taking every cap or refusing them all,
    /// and logging what it took.
    struct Ec {
        limit: Mutex<u8>,
        caps: bool,
        refusing: bool,
        written: Mutex<Vec<u32>>,
    }

    impl Charger for Ec {
        fn charge_limit(&self) -> DeviceResult<u8> {
            Ok(*self.limit.lock().unwrap())
        }

        fn set_charge_limit(&self, percent: u8) -> DeviceResult<()> {
            *self.limit.lock().unwrap() = percent;
            Ok(())
        }

        fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<()> {
            if self.refusing {
                return Err(DeviceError::Failed("no EC".into()));
            }
            self.written.lock().unwrap().push(milliamps);
            Ok(())
        }

        fn charge_current_limit_supported(&self) -> bool {
            self.caps
        }
    }

    struct Machine {
        condition: bool,
        caps: bool,
        refusing: bool,
        same_boot: Option<bool>,
    }

    const FULL: Machine = Machine {
        condition: true,
        caps: true,
        refusing: false,
        same_boot: Some(true),
    };

    struct Bench {
        battery: Battery,
        ec: Arc<Ec>,
        clock: Arc<Clock>,
    }

    fn over(machine: &Machine, store: &Arc<Memory>) -> Bench {
        let pack = Arc::new(Gauge {
            answering: machine.condition,
        });
        let ec = Arc::new(Ec {
            limit: Mutex::new(100),
            caps: machine.caps,
            refusing: machine.refusing,
            written: Mutex::new(Vec::new()),
        });
        let clock = Clock::new(machine.same_boot);
        let identity = pack.identity().unwrap();
        Bench {
            battery: Battery::new(pack, ec.clone(), clock.clone(), store.clone(), identity),
            ec,
            clock,
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
        let Bench { battery, ec, .. } = over(&FULL, &store);
        assert_eq!(ready(battery.set_charge_limit(80)), Ok(true));
        assert_eq!(*ec.limit.lock().unwrap(), 80);
        assert_eq!(ready(battery.charge_limit()), Ok(80));
    }

    #[test]
    fn a_ceiling_off_the_range_is_refused_before_the_charger_is_asked() {
        let store = Arc::new(Memory::default());
        let Bench { battery, ec, .. } = over(&FULL, &store);
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
        let Bench { battery, ec, .. } = over(&FULL, &store);
        assert_eq!(ready(battery.set_charge_current_limit(1_500)), Ok(true));
        assert_eq!(*ec.written.lock().unwrap(), [1_500]);
        assert_eq!(ready(battery.charge_current_limit()), Ok(1_500));
        assert_eq!(store.get(KEY_CURRENT_LIMIT).as_deref(), Some("1500"));
        assert!(store.get(KEY_CURRENT_LIMIT_STAMP).is_some());
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
        let Bench { battery, ec, .. } = over(&FULL, &store);
        assert!(matches!(
            ready(battery.set_charge_current_limit(0)),
            Err(DeviceError::InvalidArgs(_))
        ));
        assert!(ec.written.lock().unwrap().is_empty());
    }

    /// An EC that restarted has dropped the cap, whatever the mirror says.
    #[test]
    fn a_stamp_from_another_ec_boot_reads_as_no_limit() {
        let store = Arc::new(Memory::default());
        let Bench { battery, clock, .. } = over(&FULL, &store);
        ready(battery.set_charge_current_limit(1_500)).unwrap();
        *clock.same_boot.lock().unwrap() = Some(false);
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
    }

    /// A cap the EC could not date is still written, and forgotten: with no
    /// stamp to weigh it by, nothing could say later whether it still stands.
    #[test]
    fn an_undatable_cap_is_written_and_not_remembered() {
        let store = Arc::new(Memory::default());
        let Bench { battery, ec, .. } = over(
            &Machine {
                same_boot: None,
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
        assert_eq!(store.get(KEY_CURRENT_LIMIT_STAMP), None);
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
    }

    #[test]
    fn a_stored_zero_reads_as_no_limit() {
        let store = Arc::new(Memory::default());
        store.set(KEY_CURRENT_LIMIT, Some("0".into()));
        let Bench { battery, .. } = over(&FULL, &store);
        assert_eq!(
            ready(battery.charge_current_limit()),
            Ok(NO_CHARGE_CURRENT_LIMIT)
        );
    }
}
