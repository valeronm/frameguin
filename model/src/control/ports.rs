//! The USB-C ports: one read, which port is powering the machine, and the
//! switch each port is.

use std::rc::Rc;

use frameguin_contract::{
    DeviceResult as Result, OffRefusal, Platform, PortMask, PortReading, PortSet, PortState,
    PortsControl, off_refusal,
};

use super::present;
use crate::port::Placement;

pub struct Ports<C> {
    control: Rc<C>,
    placement: Placement,
    switchable: PortSet,
    platform: Platform,
}

impl<C: PortsControl> Ports<C> {
    pub fn new(control: Rc<C>, platform: Platform, switchable: PortSet) -> Self {
        Self {
            control,
            placement: Placement::of(platform),
            switchable,
            platform,
        }
    }

    pub async fn detect(control: &Rc<C>, platform: Platform) -> Result<Option<Self>> {
        let Some(switchable) = present(control.switchable().await)? else {
            return Ok(None);
        };
        Ok(Some(Self::new(control.clone(), platform, switchable)))
    }

    pub async fn read(&self) -> Result<Vec<PortState>> {
        self.control.ports().await
    }

    /// The ports offered a switch, fixed for the device's run.
    #[must_use]
    pub fn switchable(&self) -> PortSet {
        self.switchable
    }

    pub async fn set_enabled(&self, port: u8, enabled: bool) -> Result<()> {
        self.control.set_enabled(port, enabled).await
    }

    /// Why the hardware would refuse turning `port` off, and None where it
    /// would not or the port is already off.
    #[must_use]
    pub fn refusal(&self, ports: &[PortState], port: u8) -> Option<OffRefusal> {
        let this = ports.iter().find(|state| state.index == port)?;
        if this.mask == PortMask::Off {
            return None;
        }
        let states = ports.iter().map(|state| {
            let enabled = match state.mask {
                PortMask::Off => Some(false),
                PortMask::On(_) => Some(true),
                PortMask::Unchecked(_) => None,
            };
            (state.index, enabled)
        });
        off_refusal(self.platform, port, states)
    }

    /// Where this board's sockets are, fixed for the device's run.
    #[must_use]
    pub fn placement(&self) -> Placement {
        self.placement
    }
}

/// The port the machine is drawing its power through, by its index, and
/// None where none is. At most one answers, the EC picking among those
/// offering.
#[must_use]
pub fn powering(ports: &[PortState]) -> Option<(u8, &PortReading)> {
    ports.iter().find_map(|port| match &port.mask {
        PortMask::Off => None,
        PortMask::On(reading) | PortMask::Unchecked(reading) => {
            reading.charging.then_some((port.index, reading))
        }
    })
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use frameguin_contract::{
        DeviceError, DeviceResult as Result, OffRefusal, Platform, PortMask, PortSet, PortState,
    };

    use super::{Ports, powering};
    use crate::testing::{Machine, absent, port, reading, ready};

    fn detect(machine: &Rc<Machine>) -> Result<Option<Ports<Machine>>> {
        ready(Ports::detect(machine, Platform::Unknown))
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
        let ports = Ports::new(Machine::new(), Platform::Unknown, PortSet::default());
        let read = ready(ports.read()).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(read[0], port(0));
    }

    #[test]
    fn the_port_charging_the_machine_is_the_one_powering_it() {
        let ports: Vec<_> = (0..4).map(port).collect();
        assert_eq!(powering(&ports).map(|(index, _)| index), Some(0));
        let idle: Vec<_> = (1..4).map(port).collect();
        assert!(powering(&idle).is_none());
    }

    #[test]
    fn a_port_whose_mask_went_unread_still_powers_the_machine() {
        let unchecked = PortState {
            index: 0,
            mask: PortMask::Unchecked(reading(0)),
        };
        assert_eq!(powering(&[unchecked]).map(|(index, _)| index), Some(0));
    }

    fn off(index: u8) -> PortState {
        PortState {
            index,
            mask: PortMask::Off,
        }
    }

    fn laptop() -> Ports<Machine> {
        Ports::new(Machine::new(), Platform::Laptop13ProUltra3, PortSet::ALL)
    }

    #[test]
    fn the_last_port_on_is_refused_its_switch() {
        let ports = [off(0), port(1), off(2), off(3)];
        assert_eq!(laptop().refusal(&ports, 1), Some(OffRefusal::LastPort));
    }

    #[test]
    fn a_port_already_off_is_never_refused() {
        let ports = [off(0), port(1), off(2), off(3)];
        assert_eq!(laptop().refusal(&ports, 0), None);
    }

    #[test]
    fn every_port_on_is_refused_while_a_mask_went_unread() {
        let unchecked = PortState {
            index: 2,
            mask: PortMask::Unchecked(reading(2)),
        };
        let ports = [port(0), port(1), unchecked];
        assert_eq!(laptop().refusal(&ports, 0), Some(OffRefusal::Unchecked));
    }

    #[test]
    fn a_desktop_refuses_no_port() {
        let ports = Ports::new(Machine::new(), Platform::DesktopAmdAiMax300, PortSet::ALL);
        assert_eq!(ports.refusal(&[off(0), port(1)], 1), None);
    }

    #[test]
    fn detection_carries_the_switchable_ports() {
        let ports = detect(&Machine::new()).unwrap().expect("ports answered");
        assert!(ports.switchable().contains(3));
    }
}
