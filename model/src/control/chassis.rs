//! The chassis open switch and the input deck: their reads.

use std::rc::Rc;

use frameguin_contract::{
    ChassisControl, ChassisFeature, ChassisState, DeckState, DeviceResult as Result,
};

use super::present;

pub struct Chassis<C> {
    control: Rc<C>,
    features: Vec<ChassisFeature>,
}

impl<C: ChassisControl> Chassis<C> {
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.features().await)?.map(|features| Self {
            control: control.clone(),
            features,
        }))
    }

    #[must_use]
    pub fn has(&self, feature: ChassisFeature) -> bool {
        self.features.contains(&feature)
    }

    pub async fn read(&self) -> Result<ChassisState> {
        self.control.state().await
    }

    pub async fn deck_state(&self) -> Result<DeckState> {
        self.control.deck_state().await
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{ChassisFeature, DeckState, DeviceError};

    use super::Chassis;
    use crate::testing::{Machine, absent, ready};

    #[test]
    fn a_chassis_is_detected_with_its_features() {
        let chassis = ready(Chassis::detect(&Machine::new())).unwrap().unwrap();
        assert!(chassis.has(ChassisFeature::Deck));
        assert_eq!(ready(chassis.deck_state()), Ok(DeckState::On));
    }

    #[test]
    fn a_board_the_hardware_serves_no_chassis_for_is_absent() {
        let machine = Machine::failing(absent());
        assert!(ready(Chassis::detect(&machine)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_chassis() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(ready(Chassis::detect(&machine)).err(), Some(error));
    }
}
