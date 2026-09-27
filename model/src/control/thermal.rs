//! The EC's temperature sensors and fans: their reads.

use std::rc::Rc;

use frameguin_contract::{
    DeviceResult as Result, ThermalControl, ThermalFeature, ThermalLayout, ThermalState, Thresholds,
};

use super::present;

pub struct Thermal<C> {
    control: Rc<C>,
    features: Vec<ThermalFeature>,
    layout: ThermalLayout,
}

impl<C: ThermalControl> Thermal<C> {
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        let Some(features) = present(control.features().await)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            control: control.clone(),
            features,
            layout: control.layout().await?,
        }))
    }

    #[must_use]
    pub fn has(&self, feature: ThermalFeature) -> bool {
        self.features.contains(&feature)
    }

    #[must_use]
    pub fn layout(&self) -> &ThermalLayout {
        &self.layout
    }

    pub async fn read(&self) -> Result<ThermalState> {
        self.control.state().await
    }

    pub async fn thresholds(&self) -> Result<Vec<Thresholds>> {
        self.control.thresholds().await
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{DeviceError, ThermalFeature};

    use super::Thermal;
    use crate::testing::{Machine, absent, ready};

    #[test]
    fn a_thermal_device_is_detected_with_its_features_and_layout() {
        let thermal = ready(Thermal::detect(&Machine::new())).unwrap().unwrap();
        assert!(thermal.has(ThermalFeature::Thresholds));
        assert_eq!(thermal.layout().fans, [0]);
        assert_eq!(ready(thermal.read()).unwrap().fans[0].rpm, 2400);
    }

    #[test]
    fn a_board_the_hardware_serves_no_thermal_device_for_is_absent() {
        let machine = Machine::failing(absent());
        assert!(ready(Thermal::detect(&machine)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_thermal_device() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(ready(Thermal::detect(&machine)).err(), Some(error));
    }
}
