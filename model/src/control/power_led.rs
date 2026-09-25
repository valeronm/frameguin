//! The power button LED: a level picked from the ones the board has, and
//! behind the custom one a percentage.

use std::rc::Rc;

use frameguin_contract::{DeviceResult as Result, PowerLedControl, PowerLedLevel};

use super::present;

pub use frameguin_contract::MIN_POWER_LED_BRIGHTNESS;

/// What the LED is set to: the level in force, and the percentage the EC
/// lights it at — the one a preset resolved to, or the one dialled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub percent: u8,
    pub level: PowerLedLevel,
}

pub struct PowerLed<C> {
    control: Rc<C>,
    levels: Vec<PowerLedLevel>,
}

impl<C: PowerLedControl> PowerLed<C> {
    /// `levels` is every level the device has, in the order it answered.
    pub fn new(control: Rc<C>, levels: Vec<PowerLedLevel>) -> Self {
        Self { control, levels }
    }

    /// The levels are asked for once here, being fixed for the device's run.
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        if present(control.brightness().await)?.is_none() {
            return Ok(None);
        }
        let offered = control.levels().await?;
        Ok(Some(Self::new(control.clone(), offered)))
    }

    pub async fn read(&self) -> Result<Snapshot> {
        let (percent, level) = self.control.brightness().await?;
        Ok(Snapshot { percent, level })
    }

    pub async fn set_level(&self, level: PowerLedLevel) -> Result<()> {
        self.control.set_level(level).await
    }

    pub async fn set_brightness(&self, percent: u8) -> Result<()> {
        self.control.set_brightness(percent).await
    }

    /// The levels this board offers.
    #[must_use]
    pub fn levels(&self) -> &[PowerLedLevel] {
        &self.levels
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{DeviceError, PowerLedLevel};

    use super::{PowerLed, Snapshot};
    use crate::testing::{Machine, absent, ready};

    #[test]
    fn an_led_the_hardware_answers_for_is_detected_with_its_levels() {
        let led = ready(PowerLed::detect(&Machine::new())).unwrap().unwrap();
        assert_eq!(led.levels().len(), PowerLedLevel::ALL.len());
    }

    #[test]
    fn an_led_the_hardware_does_not_serve_is_absent() {
        let machine = Machine::failing(absent());
        assert!(ready(PowerLed::detect(&machine)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_led() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(ready(PowerLed::detect(&machine)).err(), Some(error));
    }

    #[test]
    fn a_read_takes_both_halves_from_the_hardware() {
        let led = PowerLed::new(Machine::new(), PowerLedLevel::ALL.to_vec());
        assert_eq!(
            ready(led.read()),
            Ok(Snapshot {
                percent: 55,
                level: PowerLedLevel::High
            })
        );
    }

    #[test]
    fn a_refused_write_carries_the_refusal() {
        let machine = Machine::new();
        let led = PowerLed::new(machine.clone(), PowerLedLevel::ALL.to_vec());
        machine.power_led.refuse();
        assert_eq!(
            ready(led.set_level(PowerLedLevel::Low)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert_eq!(machine.level.get(), PowerLedLevel::High);
    }
}
