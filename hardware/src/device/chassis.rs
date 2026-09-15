//! The chassis open switch, read and never set.

use std::sync::Arc;

use frameguin_wire::{ChassisControl, ChassisState, DeviceResult};

use crate::ec::{ChassisEc, Ec};

pub struct Chassis {
    ec: Arc<dyn ChassisEc>,
}

impl Chassis {
    pub(crate) fn detect(ec: &Arc<Ec>) -> Option<Self> {
        Self::new(ec.clone())
    }

    /// None where the EC refuses the read, which is how firmware without the
    /// chassis commands answers.
    pub fn new(ec: Arc<dyn ChassisEc>) -> Option<Self> {
        ec.chassis().ok().map(|_| Self { ec })
    }
}

impl ChassisControl for Chassis {
    async fn state(&self) -> DeviceResult<ChassisState> {
        self.ec.chassis()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_wire::ChassisControl;

    use super::Chassis;
    use crate::testing::{Cover, ready};

    #[test]
    fn a_chassis_the_ec_answers_for_reads_what_it_answered() {
        let chassis = Chassis::new(Arc::new(Cover::default())).expect("the EC answered");
        assert_eq!(ready(chassis.state()).unwrap(), Cover::default().state);
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
