//! The machine's USB-C ports: one device answering for all of them, read and
//! never set.

use std::sync::Arc;

use frameguin_wire::{DeviceResult, PortState, PortsControl};

use crate::ec::{Ec, PdPorts};

/// How many ports one PD controller drives. The EC numbers a port as its
/// controller times this plus which of the controller's connectors it is,
/// which is what lets the controllers it answers for bound the walk.
const PORTS_PER_CONTROLLER: u8 = 2;

pub struct Ports {
    ec: Arc<dyn PdPorts>,
    /// How many ports answered at detection. Settled once: the count is the
    /// board's, and re-probing the tail would spend a refused command on
    /// every read to learn what cannot have changed.
    count: u8,
}

impl Ports {
    /// The ports, and None where the EC answers for none — a board whose
    /// firmware has no such command included, since a port that cannot be
    /// asked about is one this device has nothing to say about.
    pub fn detect(ec: &Arc<Ec>) -> Option<Self> {
        Self::new(ec.clone())
    }

    /// The walk is bounded by the controllers rather than by where the EC
    /// starts refusing, because a board has been seen not to refuse: asked
    /// for a port past its last, one answers success and a reading of
    /// `0xFF`s, having read past its own array. So the count is what the
    /// controllers can account for, and the refusal is only a second bound
    /// under it.
    pub fn new(ec: Arc<dyn PdPorts>) -> Option<Self> {
        let ceiling = ec.pd_controllers().saturating_mul(PORTS_PER_CONTROLLER);
        // The first port refused is the count of those before it.
        let count = (0..ceiling)
            .find(|port| !matches!(ec.port_state(*port), Ok(Some(_))))
            .unwrap_or(ceiling);
        (count > 0).then_some(Self { ec, count })
    }
}

impl PortsControl for Ports {
    /// A port the EC refuses part way through the walk is left out rather
    /// than failing the read: what a caller wants is the set, and one port
    /// gone silent costs its own row and not the window.
    async fn ports(&self) -> DeviceResult<Vec<PortState>> {
        Ok((0..self.count)
            .filter_map(|port| self.ec.port_state(port).ok().flatten())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_wire::PortsControl;

    use super::Ports;
    use crate::testing::{Connectors, ready};

    #[test]
    fn the_ports_the_ec_answers_for_are_the_ports_there_are() {
        let ports = Ports::new(Arc::new(Connectors::default())).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(read[3].index, 3);
    }

    #[test]
    fn a_board_the_ec_answers_for_no_port_on_has_no_device() {
        let ec = Connectors {
            count: 0,
            ..Connectors::default()
        };
        assert!(Ports::new(Arc::new(ec)).is_none());
    }

    #[test]
    fn a_board_with_no_pd_controller_has_no_ports() {
        let ec = Connectors {
            controllers: 0,
            ..Connectors::default()
        };
        assert!(Ports::new(Arc::new(ec)).is_none());
    }

    /// The bug this bound exists for: an EC that answers for every port
    /// asked instead of refusing one past its last would otherwise be walked
    /// past the ports the board has.
    #[test]
    fn an_ec_that_refuses_nothing_is_still_held_to_its_controllers() {
        let ec = Connectors {
            refusing_none: true,
            ..Connectors::default()
        };
        let ports = Ports::new(Arc::new(ec)).expect("the ceiling still allows four");
        assert_eq!(ready(ports.ports()).unwrap().len(), 4);
    }

    /// A controller's second port can be absent — the Laptop 16's third
    /// drives one — so the refusal still bounds the walk under the ceiling.
    #[test]
    fn a_controller_driving_one_port_stops_the_walk_under_the_ceiling() {
        let ec = Connectors {
            controllers: 3,
            count: 5,
            ..Connectors::default()
        };
        let ports = Ports::new(Arc::new(ec)).expect("five ports answered");
        assert_eq!(ready(ports.ports()).unwrap().len(), 5);
    }
}
