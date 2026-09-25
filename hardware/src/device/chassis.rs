//! The chassis open switch, and the input deck's power state, read and
//! never set.

use std::sync::Arc;

use frameguin_contract::{ChassisControl, ChassisFeature, ChassisState, DeckState, DeviceResult};

use crate::ec::{ChassisEc, Ec};

pub struct Chassis {
    ec: Arc<dyn ChassisEc>,
    features: Vec<ChassisFeature>,
}

impl Chassis {
    pub(crate) fn detect(ec: &Arc<Ec>) -> Option<Self> {
        Self::new(ec.clone())
    }

    /// None where the EC refuses the read, which is how firmware without the
    /// chassis commands answers. The deck is probed by its getter's own read.
    pub fn new(ec: Arc<dyn ChassisEc>) -> Option<Self> {
        ec.chassis().ok()?;
        let mut features = Vec::new();
        if ec.deck_state().is_ok() {
            features.push(ChassisFeature::Deck);
        }
        Some(Self { ec, features })
    }
}

impl ChassisControl for Chassis {
    async fn state(&self) -> DeviceResult<ChassisState> {
        self.ec.chassis()
    }

    async fn features(&self) -> DeviceResult<Vec<ChassisFeature>> {
        Ok(self.features.clone())
    }

    async fn deck_state(&self) -> DeviceResult<DeckState> {
        self.ec.deck_state()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_contract::ChassisControl;

    use super::Chassis;
    use crate::testing::{Cover, ready};

    #[test]
    fn an_ec_without_the_deck_command_offers_no_deck() {
        let ec = Cover {
            deck: None,
            ..Cover::default()
        };
        let chassis = Chassis::new(Arc::new(ec)).expect("the EC answered");
        assert_eq!(ready(chassis.features()), Ok(Vec::new()));
    }

    #[test]
    fn an_ec_refusing_the_chassis_commands_has_no_chassis() {
        let ec = Cover {
            refusing: true,
            ..Cover::default()
        };
        assert!(Chassis::new(Arc::new(ec)).is_none());
    }
}
