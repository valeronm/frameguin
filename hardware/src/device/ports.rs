//! The machine's USB-C ports: one device answering for all of them, read and
//! never set.

use std::sync::Arc;

use frameguin_contract::{
    Board, DeviceResult, Platform, PortPartner, PortSet, PortState, PortsControl,
};

use crate::ec::{Ec, PdPorts};
use crate::pd_controller::{self, PORTS_PER_CONTROLLER};

pub struct Ports {
    ec: Arc<dyn PdPorts>,
    /// How many ports answered at detection. Settled once: the count is the
    /// board's, and re-probing the tail would spend a refused command on
    /// every read to learn what cannot have changed.
    count: u8,
    /// By the EC's controller order, and None for one the board's table does
    /// not place.
    controllers: Vec<Option<Controller>>,
}

/// A controller's (I2C port, address), and which of its reads answered at
/// detection.
#[derive(Clone, Copy)]
struct Controller {
    address: (u8, u16),
    registers: bool,
    mask: bool,
}

impl Ports {
    /// The ports, and None where the EC answers for none — a board whose
    /// firmware has no such command included, since a port that cannot be
    /// asked about is one this device has nothing to say about.
    pub(crate) fn detect(ec: &Arc<Ec>, board: &Board) -> Option<Self> {
        Self::new(ec.clone(), board.platform())
    }

    /// The walk is bounded by the controllers rather than by where the EC
    /// starts refusing, because a board has been seen not to refuse: asked
    /// for a port past its last, one answers success and a reading of
    /// `0xFF`s, having read past its own array. So the count is what the
    /// controllers can account for, and the refusal is only a second bound
    /// under it.
    pub fn new(ec: Arc<dyn PdPorts>, platform: Platform) -> Option<Self> {
        let controllers = pd_controller::controllers(platform);
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
                controllers.get(usize::from(c)).map(|&address| Controller {
                    address,
                    registers: ec.port_registers(c * PORTS_PER_CONTROLLER, address).is_ok(),
                    mask: ec.port_enable(address).is_ok(),
                })
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
    /// not read leave the port without them for that read rather than
    /// costing the port.
    async fn ports(&self, controller_ports: PortSet) -> DeviceResult<Vec<PortState>> {
        let masks: Vec<_> = self
            .controllers
            .iter()
            .map(|controller| {
                controller
                    .filter(|controller| controller.mask)
                    .and_then(|controller| self.ec.port_enable(controller.address).ok())
            })
            .collect();
        Ok((0..self.count)
            .filter_map(|port| self.ec.port_state(port).ok().flatten())
            .map(|mut state| {
                let c = usize::from(state.index / PORTS_PER_CONTROLLER);
                state.enabled = masks
                    .get(c)
                    .copied()
                    .flatten()
                    .map(|mask| pd_controller::enabled(mask, state.index));
                if controller_ports.contains(state.index)
                    && state.partner != PortPartner::Nothing
                    && !state.turned_off()
                    && let Some(&Some(controller)) = self.controllers.get(c)
                    && controller.registers
                {
                    state.registers = self.ec.port_registers(state.index, controller.address).ok();
                }
                state
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_contract::{
        Cable, CableLatency, CableSpeed, PdContract, Platform, PortRegisters, PortSet,
        PortsControl, SupplyKind,
    };

    use super::Ports;
    use crate::pd_controller;
    use crate::testing::{Connectors, ready};

    const LAPTOP_13_PRO: Platform = Platform::Laptop13ProUltra3;

    fn laptop_13_pro() -> &'static [(u8, u16)] {
        pd_controller::controllers(LAPTOP_13_PRO)
    }

    #[test]
    fn the_ports_the_ec_answers_for_are_the_ports_there_are() {
        let ports = Ports::new(Arc::new(Connectors::default()), LAPTOP_13_PRO)
            .expect("four ports answered");
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(read[3].index, 3);
    }

    #[test]
    fn a_board_the_ec_answers_for_no_port_on_has_no_device() {
        let ec = Connectors {
            count: 0,
            ..Connectors::default()
        };
        assert!(Ports::new(Arc::new(ec), LAPTOP_13_PRO).is_none());
    }

    #[test]
    fn a_board_with_no_pd_controller_has_no_ports() {
        let ec = Connectors {
            controllers: 0,
            ..Connectors::default()
        };
        assert!(Ports::new(Arc::new(ec), LAPTOP_13_PRO).is_none());
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
        let ports = Ports::new(Arc::new(ec), LAPTOP_13_PRO).expect("the ceiling still allows four");
        assert_eq!(ready(ports.ports(PortSet::ALL)).unwrap().len(), 4);
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
        let ports =
            Ports::new(Arc::new(ec), Platform::Laptop16AmdAi300).expect("five ports answered");
        assert_eq!(ready(ports.ports(PortSet::ALL)).unwrap().len(), 5);
    }

    fn marked() -> Cable {
        Cable {
            speed: Some(CableSpeed::Usb2),
            milliamps: Some(5000),
            epr: true,
            latency: Some(CableLatency::Under30Ns),
            active: false,
        }
    }

    fn charging() -> PortRegisters {
        PortRegisters {
            cable: Some(marked()),
            pd: Some(PdContract {
                kind: SupplyKind::Fixed,
                peak: None,
                power_limited: None,
                capability_mismatch: false,
            }),
            measured_millivolts: 20_100,
        }
    }

    #[test]
    fn an_attached_port_carries_its_cable_contract_and_voltage() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec, LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read[0].registers, Some(charging()));
    }

    #[test]
    fn an_empty_port_is_never_asked_about_its_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read[1].registers, None);
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
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("the ports still answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(read[0].registers, None);
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
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read[0].registers, Some(charging()));
        assert_eq!(read[2].registers, None);
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
        let ports = Ports::new(ec.clone(), Platform::Unknown).expect("the ports still answered");
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read[0].registers, None);
        assert!(ec.registers_read.lock().unwrap().is_empty());
    }

    #[test]
    fn an_attached_port_on_the_second_controller_is_read_from_it() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(2),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read[2].registers, Some(charging()));
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
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(PortSet::default())).unwrap();
        assert_eq!(read[0].registers, None);
        assert!(ec.registers_read.lock().unwrap().is_empty());
    }

    #[test]
    fn each_bit_of_a_controllers_mask_is_one_of_its_ports() {
        let ec = Connectors {
            masks: vec![(laptop_13_pro()[1], 0b01)],
            ..Connectors::default()
        };
        let ports = Ports::new(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        let enabled: Vec<_> = read.iter().map(|state| state.enabled).collect();
        assert_eq!(enabled, [Some(true), Some(true), Some(true), Some(false)]);
    }

    #[test]
    fn a_stub_turning_one_port_off_leaves_its_neighbour_on() {
        let ec = Connectors::default().with_port_off(LAPTOP_13_PRO, 3);
        let ports = Ports::new(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read[2].enabled, Some(true));
        assert_eq!(read[3].enabled, Some(false));
    }

    #[test]
    fn a_port_turned_off_is_never_asked_about_its_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(3),
            masks: vec![(laptop_13_pro()[1], 0b01)],
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(PortSet::of(3))).unwrap();
        assert_eq!(read[3].registers, None);
        assert!(ec.registers_read.lock().unwrap().is_empty());
    }

    #[test]
    fn a_controller_refusing_its_mask_leaves_only_its_own_ports_unknown() {
        let ec = Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        };
        let ports = Ports::new(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(read[0].enabled, Some(true));
        assert_eq!(read[2].enabled, None);
    }

    #[test]
    fn a_mask_refused_at_detection_is_never_asked_again() {
        let ec = Arc::new(Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.masks_read.lock().unwrap().clear();
        ready(ports.ports(PortSet::ALL)).unwrap();
        assert_eq!(*ec.masks_read.lock().unwrap(), vec![laptop_13_pro()[0]]);
    }

    #[test]
    fn a_board_with_no_controller_table_knows_no_mask() {
        let ports = Ports::new(Arc::new(Connectors::default()), Platform::Unknown)
            .expect("the ports still answered");
        let read = ready(ports.ports(PortSet::ALL)).unwrap();
        assert!(read.iter().all(|state| state.enabled.is_none()));
    }

    #[test]
    fn only_the_ports_asked_for_are_read() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(2),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.registers_read.lock().unwrap().clear();
        let read = ready(ports.ports(PortSet::of(2))).unwrap();
        assert_eq!(read[0].registers, None);
        assert_eq!(read[2].registers, Some(charging()));
        assert_eq!(
            *ec.registers_read.lock().unwrap(),
            vec![(2, laptop_13_pro()[1])]
        );
    }
}
