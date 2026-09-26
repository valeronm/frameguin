//! The battery: the pack and the two limits that shape its charging. What
//! the extender holds the pack at is [`extender`]'s.

pub mod extender;

use std::cell::Cell;
use std::num::NonZeroU32;
use std::rc::Rc;

use frameguin_contract::{
    BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, ChargeCurrentLimit,
    DeviceResult as Result, ExtenderState,
};

use super::present;

pub struct Battery<C> {
    control: Rc<C>,
    features: Vec<BatteryFeature>,
    /// The last reading's; the pack cannot be swapped while the machine runs.
    charge_speeds: Cell<Option<ChargeSpeeds>>,
}

impl<C: BatteryControl> Battery<C> {
    pub fn new(control: Rc<C>, features: Vec<BatteryFeature>) -> Self {
        Self {
            control,
            features,
            charge_speeds: Cell::new(None),
        }
    }

    /// Probed by the features, which are wanted anyway and fixed for the
    /// device's run — a read of the block here would only be repeated by the
    /// first fill.
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.features().await)?.map(|features| Self::new(control.clone(), features)))
    }

    #[must_use]
    pub fn has(&self, feature: BatteryFeature) -> bool {
        self.features.contains(&feature)
    }

    /// Every feature this battery offers, for a front-end that keeps its own
    /// copy.
    #[must_use]
    pub fn features(&self) -> &[BatteryFeature] {
        &self.features
    }

    pub async fn read(&self) -> Result<BatteryInfo> {
        let info = self.control.info().await?;
        self.charge_speeds
            .set(Some(ChargeSpeeds::new(info.design_capacity)));
        Ok(info)
    }

    /// None until a reading has arrived, with nothing to fall back to and
    /// nothing that should be: every rate offered or sent is a fraction of the
    /// pack's capacity, so a second guess would be a second answer to "how
    /// fast is full speed".
    #[must_use]
    pub fn charge_speeds(&self) -> Option<ChargeSpeeds> {
        self.charge_speeds.get()
    }

    pub async fn condition(&self) -> Result<BatteryCondition> {
        self.control.condition().await
    }

    pub async fn charge_limit(&self) -> Result<u8> {
        self.control.charge_limit().await
    }

    /// True where the hardware was written, which is what earns a toast.
    pub async fn set_charge_limit(&self, percent: u8) -> Result<bool> {
        self.control.set_charge_limit(percent).await
    }

    pub async fn charge_current_limit(&self) -> Result<ChargeCurrentLimit> {
        self.control.charge_current_limit().await
    }

    pub async fn set_charge_current_limit(&self, limit: ChargeCurrentLimit) -> Result<bool> {
        self.control.set_charge_current_limit(limit).await
    }

    pub async fn extender(&self) -> Result<ExtenderState> {
        self.control.extender().await
    }
}

/// The lowest ceiling the custom slider offers is the lowest the daemon
/// accepts: a slider reaching below it would offer a write it refuses.
pub use frameguin_contract::MIN_CHARGE_LIMIT;

/// The slowest the custom slider will ask for. The EC takes anything above
/// zero, but a limit this side of it charges so slowly that it reads as a
/// fault rather than a setting.
pub const MIN_CUSTOM_CHARGE_MA: NonZeroU32 = NonZeroU32::new(100).unwrap();

/// Never below the slowest current the custom slider offers.
#[must_use]
pub fn custom_charge_ma(milliamps: u32) -> NonZeroU32 {
    NonZeroU32::new(milliamps)
        .unwrap_or(MIN_CUSTOM_CHARGE_MA)
        .max(MIN_CUSTOM_CHARGE_MA)
}

/// What the custom slider rounds to. A `GtkScale` is continuous while
/// dragged — its step increment reaches only keys and the wheel — so without
/// this a drag lands on a value like 984 mA that the row then displays as
/// "1.0 A", reporting a current nobody chose.
pub const CUSTOM_CHARGE_STEP_MA: u32 = 100;

/// A pack's 1C design current, its design capacity read as a current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeSpeeds {
    design_capacity: u32,
}

impl ChargeSpeeds {
    /// From a pack's design capacity in mAh.
    #[must_use]
    pub const fn new(design_capacity: u32) -> Self {
        Self { design_capacity }
    }

    #[must_use]
    pub const fn design_capacity(self) -> u32 {
        self.design_capacity
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{BatteryFeature, DeviceError};

    use super::Battery;
    use crate::fixtures::SPEEDS;
    use crate::testing::{Machine, absent, cap, ready};

    #[test]
    fn a_pack_the_hardware_answers_for_is_detected_with_its_features() {
        let battery = ready(Battery::detect(&Machine::new())).unwrap().unwrap();
        assert!(battery.has(BatteryFeature::ChargeLimit));
        assert!(!battery.has(BatteryFeature::Condition));
    }

    #[test]
    fn a_pack_the_hardware_does_not_serve_is_absent() {
        let machine = Machine::failing(absent());
        assert!(ready(Battery::detect(&machine)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_pack() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(ready(Battery::detect(&machine)).err(), Some(error));
    }

    #[test]
    fn a_write_reaches_the_hardware_and_a_read_sees_it() {
        let battery = Battery::new(Machine::new(), Vec::new());
        assert_eq!(ready(battery.set_charge_limit(80)), Ok(true));
        assert_eq!(ready(battery.charge_limit()), Ok(80));
        assert_eq!(
            ready(battery.set_charge_current_limit(cap(1_160))),
            Ok(true)
        );
        assert_eq!(ready(battery.charge_current_limit()), Ok(cap(1_160)));
    }

    #[test]
    fn a_refused_write_carries_the_refusal() {
        let machine = Machine::new();
        let battery = Battery::new(machine.clone(), Vec::new());
        machine.battery.refuse();
        assert_eq!(
            ready(battery.set_charge_limit(80)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert_eq!(machine.limit.get(), 100);
    }

    #[test]
    fn a_reading_is_what_the_speeds_are_taken_from() {
        let battery = Battery::new(Machine::new(), Vec::new());
        assert_eq!(battery.charge_speeds(), None);
        ready(battery.read()).unwrap();
        assert_eq!(battery.charge_speeds(), Some(SPEEDS));
    }

    #[test]
    fn a_failed_reading_leaves_no_speeds() {
        let error = DeviceError::Failed("no reply".into());
        let battery = Battery::new(Machine::failing(error), Vec::new());
        assert!(ready(battery.read()).is_err());
        assert_eq!(battery.charge_speeds(), None);
    }
}
