//! The USB-C ports: one read, and which port is powering the machine.

use std::rc::Rc;

use frameguin_contract::{DeviceResult as Result, PortSet, PortState, PortsControl};

use super::present;
use crate::port::Placement;

pub struct Ports<C> {
    control: Rc<C>,
    placement: Placement,
}

impl<C: PortsControl> Ports<C> {
    pub fn new(control: Rc<C>, placement: Placement) -> Self {
        Self { control, placement }
    }

    pub async fn detect(control: &Rc<C>, placement: Placement) -> Result<Option<Self>> {
        Ok(present(control.ports(PortSet::default()).await)?
            .map(|_| Self::new(control.clone(), placement)))
    }

    pub async fn read(&self, controller_ports: PortSet) -> Result<Vec<PortState>> {
        self.control.ports(controller_ports).await
    }

    /// Where this board's sockets are, fixed for the device's run.
    #[must_use]
    pub fn placement(&self) -> Placement {
        self.placement
    }
}

/// The port the machine is drawing its power through, and None where none
/// is. At most one answers, the EC picking among those offering.
#[must_use]
pub fn powering(ports: &[PortState]) -> Option<&PortState> {
    ports.iter().find(|port| port.charging)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use frameguin_contract::{DeviceError, DeviceResult as Result, PortSet};

    use super::{Ports, powering};
    use crate::port::Placement;
    use crate::testing::{Machine, absent, port, ready};

    fn detect(machine: &Rc<Machine>) -> Result<Option<Ports<Machine>>> {
        ready(Ports::detect(machine, Placement::default()))
    }

    #[test]
    fn ports_the_hardware_answers_for_are_detected() {
        assert!(detect(&Machine::new()).unwrap().is_some());
    }

    #[test]
    fn a_board_the_hardware_serves_no_ports_for_is_absent() {
        let machine = Machine::failing(absent());
        assert!(detect(&machine).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_set_of_ports() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(detect(&machine).err(), Some(error));
    }

    #[test]
    fn a_read_carries_every_port() {
        let ports = Ports::new(Machine::new(), Placement::default());
        let read = ready(ports.read(PortSet::default())).unwrap();
        assert_eq!(read.len(), 4);
        assert!(read[0].charging);
    }

    #[test]
    fn the_port_charging_the_machine_is_the_one_powering_it() {
        let ports: Vec<_> = (0..4).map(port).collect();
        assert_eq!(powering(&ports).map(|p| p.index), Some(0));
        let idle: Vec<_> = (1..4).map(port).collect();
        assert!(powering(&idle).is_none());
    }
}
