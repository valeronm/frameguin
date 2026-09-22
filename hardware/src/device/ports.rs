//! The machine's USB-C ports: one device answering for all of them, read and
//! never set.

use std::sync::Arc;

use frameguin_wire::{Board, DeviceResult, PortPartner, PortState, PortsControl};

use crate::cable::{self, PORTS_PER_CONTROLLER};
use crate::ec::{Ec, PdPorts};

pub struct Ports {
    ec: Arc<dyn PdPorts>,
    /// How many ports answered at detection. Settled once: the count is the
    /// board's, and re-probing the tail would spend a refused command on
    /// every read to learn what cannot have changed.
    count: u8,
    /// Each controller's (I2C port, address), by the EC's controller order,
    /// and None for one that did not answer a register read at detection.
    controllers: Vec<Option<(u8, u16)>>,
}

impl Ports {
    /// The ports, and None where the EC answers for none — a board whose
    /// firmware has no such command included, since a port that cannot be
    /// asked about is one this device has nothing to say about.
    pub(crate) fn detect(ec: &Arc<Ec>, board: &Board) -> Option<Self> {
        Self::new(ec.clone(), cable::controllers(board.platform()))
    }

    /// The walk is bounded by the controllers rather than by where the EC
    /// starts refusing, because a board has been seen not to refuse: asked
    /// for a port past its last, one answers success and a reading of
    /// `0xFF`s, having read past its own array. So the count is what the
    /// controllers can account for, and the refusal is only a second bound
    /// under it.
    pub fn new(ec: Arc<dyn PdPorts>, controllers: &'static [(u8, u16)]) -> Option<Self> {
        let ceiling = ec.pd_controllers().saturating_mul(PORTS_PER_CONTROLLER);
        // The first port refused is the count of those before it.
        let count = (0..ceiling)
            .find(|port| !matches!(ec.port_state(*port), Ok(Some(_))))
            .unwrap_or(ceiling);
        if count == 0 {
            return None;
        }
        let controllers = (0..count.div_ceil(PORTS_PER_CONTROLLER))
            .map(|c| {
                controllers
                    .get(usize::from(c))
                    .copied()
                    .filter(|&address| ec.port_registers(c * PORTS_PER_CONTROLLER, address).is_ok())
            })
            .collect();
        Some(Self {
            ec,
            count,
            controllers,
        })
    }
}

impl PortsControl for Ports {
    /// A port the EC refuses part way through the walk is left out rather
    /// than failing the read: what a caller wants is the set, and one port
    /// gone silent costs its own row and not the window. Registers that will
    /// not read leave the cable unknown and the voltage unread for that read
    /// rather than costing the port.
    async fn ports(&self, controller_ports: u8) -> DeviceResult<Vec<PortState>> {
        Ok((0..self.count)
            .filter_map(|port| self.ec.port_state(port).ok().flatten())
            .map(|mut state| {
                if controller_ports
                    .checked_shr(u32::from(state.index))
                    .is_some_and(|bits| bits & 1 == 1)
                    && state.partner != PortPartner::Nothing
                    && let Some(&Some(address)) = self
                        .controllers
                        .get(usize::from(state.index / PORTS_PER_CONTROLLER))
                {
                    let registers = self
                        .ec
                        .port_registers(state.index, address)
                        .unwrap_or_default();
                    state.cable = registers.cable;
                    state.measured_millivolts = registers.measured_millivolts;
                }
                state
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_wire::{Cable, CableMarking, Platform, PortsControl};

    use super::Ports;
    use crate::cable::{self, PortRegisters};
    use crate::testing::{Connectors, ready};

    fn laptop_13_pro() -> &'static [(u8, u16)] {
        cable::controllers(Platform::Laptop13ProUltra3)
    }

    #[test]
    fn the_ports_the_ec_answers_for_are_the_ports_there_are() {
        let ports = Ports::new(Arc::new(Connectors::default()), laptop_13_pro())
            .expect("four ports answered");
        let read = ready(ports.ports(u8::MAX)).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(read[3].index, 3);
    }

    #[test]
    fn a_board_the_ec_answers_for_no_port_on_has_no_device() {
        let ec = Connectors {
            count: 0,
            ..Connectors::default()
        };
        assert!(Ports::new(Arc::new(ec), laptop_13_pro()).is_none());
    }

    #[test]
    fn a_board_with_no_pd_controller_has_no_ports() {
        let ec = Connectors {
            controllers: 0,
            ..Connectors::default()
        };
        assert!(Ports::new(Arc::new(ec), laptop_13_pro()).is_none());
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
        let ports =
            Ports::new(Arc::new(ec), laptop_13_pro()).expect("the ceiling still allows four");
        assert_eq!(ready(ports.ports(u8::MAX)).unwrap().len(), 4);
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
        let ports = Ports::new(Arc::new(ec), cable::controllers(Platform::Laptop16AmdAi300))
            .expect("five ports answered");
        assert_eq!(ready(ports.ports(u8::MAX)).unwrap().len(), 5);
    }

    fn marked() -> Cable {
        Cable {
            marking: CableMarking::Marked,
            milliamps: 5000,
            ..Cable::default()
        }
    }

    fn charging() -> PortRegisters {
        PortRegisters {
            cable: marked(),
            measured_millivolts: 20_100,
        }
    }

    #[test]
    fn an_attached_port_carries_its_cable_and_voltage() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec, laptop_13_pro()).expect("four ports answered");
        let read = ready(ports.ports(u8::MAX)).unwrap();
        assert_eq!(read[0].cable, marked());
        assert_eq!(read[0].measured_millivolts, 20_100);
    }

    #[test]
    fn an_empty_port_is_never_asked_about_its_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), laptop_13_pro()).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(u8::MAX)).unwrap();
        assert_eq!(read[1].cable, Cable::default());
        assert_eq!(
            *ec.registers_read.lock().unwrap(),
            vec![(0, laptop_13_pro()[0])]
        );
    }

    #[test]
    fn a_board_whose_cable_probe_fails_never_reads_a_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            refusing_registers: laptop_13_pro().to_vec(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), laptop_13_pro()).expect("the ports still answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(u8::MAX)).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(read[0].cable, Cable::default());
        assert!(ec.registers_read.lock().unwrap().is_empty());
    }

    #[test]
    fn a_controller_failing_the_cable_probe_costs_only_its_own_ports() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(2),
            refusing_registers: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), laptop_13_pro()).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(u8::MAX)).unwrap();
        assert_eq!(read[0].cable, marked());
        assert_eq!(read[2].cable, Cable::default());
        assert_eq!(
            *ec.registers_read.lock().unwrap(),
            vec![(0, laptop_13_pro()[0])]
        );
    }

    #[test]
    fn a_board_with_no_controller_table_never_reads_a_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), &[]).expect("the ports still answered");
        let read = ready(ports.ports(u8::MAX)).unwrap();
        assert_eq!(read[0].cable, Cable::default());
        assert!(ec.registers_read.lock().unwrap().is_empty());
    }

    #[test]
    fn an_attached_port_on_the_second_controller_is_read_from_it() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(2),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), laptop_13_pro()).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(u8::MAX)).unwrap();
        assert_eq!(read[2].cable, marked());
        assert!(
            ec.registers_read
                .lock()
                .unwrap()
                .contains(&(2, laptop_13_pro()[1]))
        );
    }

    #[test]
    fn a_read_not_asking_for_the_controller_reads_none() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), laptop_13_pro()).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(0)).unwrap();
        assert_eq!(read[0].cable, Cable::default());
        assert!(ec.registers_read.lock().unwrap().is_empty());
    }

    #[test]
    fn only_the_ports_asked_for_are_read() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(2),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), laptop_13_pro()).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(1 << 2)).unwrap();
        assert_eq!(read[0].cable, Cable::default());
        assert_eq!(read[2].cable, marked());
        assert_eq!(
            *ec.registers_read.lock().unwrap(),
            vec![(2, laptop_13_pro()[1])]
        );
    }
}
