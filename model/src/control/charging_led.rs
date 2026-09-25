//! The charging LED: one switch, and which side of the chassis is lit.

use std::rc::Rc;

use frameguin_contract::{
    ChargingLedControl, ChargingLedFeature, ChargingLedSide, DeviceResult as Result,
};

use super::present;

pub struct ChargingLed<C> {
    control: Rc<C>,
    features: Vec<ChargingLedFeature>,
}

impl<C: ChargingLedControl> ChargingLed<C> {
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        if present(control.enabled().await)?.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            control: control.clone(),
            features: control.features().await?,
        }))
    }

    #[must_use]
    pub fn has(&self, feature: ChargingLedFeature) -> bool {
        self.features.contains(&feature)
    }

    pub async fn read(&self) -> Result<bool> {
        self.control.enabled().await
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<()> {
        self.control.set_enabled(enabled).await
    }

    pub async fn side(&self) -> Result<ChargingLedSide> {
        self.control.side().await
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{ChargingLedFeature, ChargingLedSide, DeviceError};

    use super::ChargingLed;
    use crate::testing::{Machine, absent, ready};

    #[test]
    fn a_led_the_hardware_answers_for_is_detected_with_its_features() {
        let led = ready(ChargingLed::detect(&Machine::new()))
            .unwrap()
            .expect("the board answered");
        assert!(led.has(ChargingLedFeature::Side));
        assert_eq!(ready(led.side()), Ok(ChargingLedSide::Left));
    }

    #[test]
    fn a_led_the_hardware_does_not_serve_is_absent() {
        let machine = Machine::failing(absent());
        assert!(ready(ChargingLed::detect(&machine)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_led() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(ready(ChargingLed::detect(&machine)).err(), Some(error));
    }

    #[test]
    fn a_write_reaches_the_hardware_and_a_refusal_leaves_it() {
        let machine = Machine::new();
        let led = ready(ChargingLed::detect(&machine)).unwrap().unwrap();
        ready(led.set_enabled(false)).unwrap();
        assert_eq!(ready(led.read()), Ok(false));
        machine.charging_led.refuse();
        assert_eq!(
            ready(led.set_enabled(true)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert_eq!(ready(led.read()), Ok(false));
    }
}
