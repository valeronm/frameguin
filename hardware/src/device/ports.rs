//! The machine's USB-C ports: one device answering for all of them, and the
//! switch each controller's port mask is.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use frameguin_contract::{
    Board, DeviceError, DeviceResult, OffRefusal, Platform, PortMask, PortSet, PortState,
    PortsControl, off_refusal,
};

use crate::ec::{Ec, PdPorts};
use crate::pd_controller::{self, PORTS_PER_CONTROLLER, controller_of};

/// How long a write waits for the controller's mask to show it.
const SETTLE: Duration = Duration::from_secs(1);
const SETTLE_POLL: Duration = Duration::from_millis(20);

pub struct Ports {
    ec: Arc<dyn PdPorts>,
    /// How many ports answered at detection. Settled once: the count is the
    /// board's, and re-probing the tail would spend a refused command on
    /// every read to learn what cannot have changed.
    count: u8,
    /// By the EC's controller order, and None for one the board's table does
    /// not place.
    controllers: Vec<Option<Controller>>,
    platform: Platform,
}

/// A controller's (I2C port, address), and what answered of it at detection.
struct Controller {
    address: (u8, u16),
    /// Decides which ports are offered a switch, and never what a write is
    /// checked against.
    mask: bool,
    /// False where the registers did not read, leaving the controller's ports
    /// on the EC's entry, which stops changing while the controller has the
    /// port disabled and changes again only on its next event after.
    direct: bool,
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
                let &address = controllers.get(usize::from(c))?;
                Some(Controller {
                    address,
                    mask: ec.port_enable(address).is_ok(),
                    direct: ec
                        .port_controller(c * PORTS_PER_CONTROLLER, address)
                        .is_ok(),
                })
            })
            .collect();
        Some(Self {
            ec,
            count,
            controllers,
            platform,
        })
    }

    /// Each controller's mask by the EC's controller order, and None for one
    /// whose mask did not read.
    fn masks(&self) -> Vec<Option<u8>> {
        self.controllers
            .iter()
            .map(|controller| {
                let controller = controller.as_ref()?;
                self.ec.port_enable(controller.address).ok()
            })
            .collect()
    }

    fn controller(&self, port: u8) -> Option<&Controller> {
        if port >= self.count {
            return None;
        }
        self.controllers.get(controller_of(port))?.as_ref()
    }
}

/// Whether `port`'s controller has it enabled, and None where its mask did
/// not read.
fn enabled_of(masks: &[Option<u8>], port: u8) -> Option<bool> {
    let mask = masks.get(controller_of(port)).copied().flatten()?;
    Some(pd_controller::enabled(mask, port))
}

impl PortsControl for Ports {
    /// A port on that the EC refuses part way through the walk is left out
    /// rather than failing the read: what a caller wants is the set, and one
    /// port gone silent costs its own row and not the window. A controller
    /// that does not answer for its registers leaves the port on the EC's
    /// entry for that read.
    async fn ports(&self) -> DeviceResult<Vec<PortState>> {
        let masks = self.masks();
        Ok((0..self.count)
            .filter_map(|index| {
                let enabled = enabled_of(&masks, index);
                if enabled == Some(false) {
                    return Some(PortState {
                        index,
                        mask: PortMask::Off,
                    });
                }
                let entry = self.ec.port_state(index).ok().flatten()?;
                let reading = match self.controller(index) {
                    Some(controller) if controller.direct => {
                        match self.ec.port_controller(index, controller.address) {
                            Ok(read) => read.over(&entry),
                            Err(_) => entry,
                        }
                    }
                    _ => entry,
                };
                let mask = if enabled.is_some() {
                    PortMask::On(reading)
                } else {
                    PortMask::Unchecked(reading)
                };
                Some(PortState { index, mask })
            })
            .collect())
    }

    /// Every controller whose mask read at detection: `PDPORT_ENABLE` is one
    /// register of the host-interface map every `CCGx` shares, so a mask that
    /// reads is one that takes a write.
    async fn switchable(&self) -> DeviceResult<PortSet> {
        Ok((0..self.count)
            .filter(|&port| {
                self.controller(port)
                    .is_some_and(|controller| controller.mask)
            })
            .collect())
    }

    async fn set_enabled(&self, port: u8, enabled: bool) -> DeviceResult<()> {
        let Some(controller) = self.controller(port) else {
            return Err(DeviceError::InvalidArgs(format!(
                "port {port} has no switch"
            )));
        };
        let unanswered = || {
            DeviceError::Failed(format!(
                "port {port}'s controller did not answer for its mask"
            ))
        };
        // Turning off weighs every controller's mask, its own among them.
        let masks = (!enabled).then(|| self.masks());
        let mask = match &masks {
            Some(masks) => masks
                .get(controller_of(port))
                .copied()
                .flatten()
                .ok_or_else(unanswered)?,
            None => self
                .ec
                .port_enable(controller.address)
                .map_err(|_| unanswered())?,
        };
        if pd_controller::enabled(mask, port) == enabled {
            return Ok(());
        }
        if let Some(masks) = &masks {
            let states = (0..self.count).map(|index| (index, enabled_of(masks, index)));
            match off_refusal(self.platform, port, states) {
                Some(OffRefusal::LastPort) => {
                    return Err(DeviceError::InvalidArgs(
                        "the last port on can't be turned off to allow charging".into(),
                    ));
                }
                Some(OffRefusal::Unchecked) => {
                    return Err(DeviceError::Failed(
                        "a port can't be checked, so none can be turned off".into(),
                    ));
                }
                None => {}
            }
        }
        let bit = pd_controller::enable_bit(port);
        let mask = if enabled { mask | bit } else { mask & !bit };
        self.ec.set_port_enable(controller.address, mask)?;
        // A controller turning a port off goes on reading it enabled for tens
        // of milliseconds after taking the write.
        let deadline = Instant::now() + SETTLE;
        while Instant::now() < deadline
            && self
                .ec
                .port_enable(controller.address)
                .is_ok_and(|read| pd_controller::enabled(read, port) != enabled)
        {
            thread::sleep(SETTLE_POLL);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_contract::{
        Cable, CableLatency, CableSpeed, DeviceError, PdContract, Platform, PortMask, PortPartner,
        PortReading, PortRegisters, PortState, PortsControl, SupplyKind,
    };

    use super::Ports;
    use crate::pd_controller;
    use crate::testing::{Connectors, ready};

    const LAPTOP_13_PRO: Platform = Platform::Laptop13ProUltra3;

    fn laptop_13_pro() -> &'static [(u8, u16)] {
        pd_controller::controllers(LAPTOP_13_PRO)
    }

    fn on(state: &PortState) -> &PortReading {
        let PortMask::On(reading) = &state.mask else {
            panic!("port {} is not on: {:?}", state.index, state.mask);
        };
        reading
    }

    #[test]
    fn the_ports_the_ec_answers_for_are_the_ports_there_are() {
        let ports = Ports::new(Arc::new(Connectors::default()), LAPTOP_13_PRO)
            .expect("four ports answered");
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
        let ports =
            Ports::new(Arc::new(ec), Platform::Laptop16AmdAi300).expect("five ports answered");
        assert_eq!(ready(ports.ports()).unwrap().len(), 5);
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
        let read = ready(ports.ports()).unwrap();
        assert_eq!(on(&read[0]).registers, Some(charging()));
    }

    #[test]
    fn an_empty_port_carries_no_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec, LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert_eq!(on(&read[1]).registers, None);
    }

    #[test]
    fn a_board_whose_controllers_fail_their_probe_reads_no_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            refusing_controllers: laptop_13_pro().to_vec(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("the ports still answered");
        ec.controller_reads.lock().unwrap().clear();
        let read = ready(ports.ports()).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(on(&read[0]).registers, None);
        assert!(ec.controller_reads.lock().unwrap().is_empty());
    }

    #[test]
    fn a_controller_failing_its_probe_costs_only_its_own_ports() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(2),
            refusing_controllers: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.controller_reads.lock().unwrap().clear();
        let read = ready(ports.ports()).unwrap();
        assert_eq!(on(&read[0]).registers, Some(charging()));
        assert_eq!(on(&read[2]).registers, None);
        assert!(
            ec.controller_reads
                .lock()
                .unwrap()
                .iter()
                .all(|&(_, controller)| controller == laptop_13_pro()[0])
        );
    }

    #[test]
    fn a_board_with_no_controller_table_never_reads_a_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), Platform::Unknown).expect("the ports still answered");
        let read = ready(ports.ports()).unwrap();
        assert!(
            matches!(&read[0].mask, PortMask::Unchecked(reading) if reading.registers.is_none())
        );
        assert!(ec.controller_reads.lock().unwrap().is_empty());
    }

    #[test]
    fn an_attached_port_on_the_second_controller_is_read_from_it() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(2),
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.controller_reads.lock().unwrap().clear();
        let read = ready(ports.ports()).unwrap();
        assert_eq!(on(&read[2]).registers, Some(charging()));
        assert!(
            ec.controller_reads
                .lock()
                .unwrap()
                .contains(&(2, laptop_13_pro()[1]))
        );
    }

    #[test]
    fn each_bit_of_a_controllers_mask_is_one_of_its_ports() {
        let ec = Connectors::default().with_port_off(LAPTOP_13_PRO, 3);
        let ports = Ports::new(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert!(
            read[..3]
                .iter()
                .all(|state| matches!(state.mask, PortMask::On(_)))
        );
        assert_eq!(read[3].mask, PortMask::Off);
    }

    #[test]
    fn a_stub_turning_one_port_off_leaves_its_neighbour_on() {
        let ec = Connectors::default().with_port_off(LAPTOP_13_PRO, 3);
        let ports = Ports::new(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert!(matches!(read[2].mask, PortMask::On(_)));
        assert_eq!(read[3].mask, PortMask::Off);
    }

    #[test]
    fn a_port_turned_off_is_never_asked_about_its_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(3),
            ..Connectors::default().with_port_off(LAPTOP_13_PRO, 3)
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.controller_reads.lock().unwrap().clear();
        let read = ready(ports.ports()).unwrap();
        assert_eq!(read[3].mask, PortMask::Off);
        assert!(
            ec.controller_reads
                .lock()
                .unwrap()
                .iter()
                .all(|&(port, _)| port != 3)
        );
    }

    #[test]
    fn a_controller_refusing_its_mask_leaves_only_its_own_ports_unknown() {
        let ec = Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        };
        let ports = Ports::new(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert!(matches!(read[0].mask, PortMask::On(_)));
        assert!(matches!(read[2].mask, PortMask::Unchecked(_)));
    }

    #[test]
    fn a_mask_refused_at_detection_is_asked_again_on_every_read() {
        let ec = Arc::new(Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.masks_read.lock().unwrap().clear();
        ready(ports.ports()).unwrap();
        assert_eq!(*ec.masks_read.lock().unwrap(), laptop_13_pro());
    }

    #[test]
    fn a_write_to_a_port_offered_no_switch_asks_its_controller() {
        let ec = Arc::new(Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.masks_read.lock().unwrap().clear();
        assert!(matches!(
            ready(ports.set_enabled(2, true)),
            Err(DeviceError::Failed(_))
        ));
        assert_eq!(*ec.masks_read.lock().unwrap(), vec![laptop_13_pro()[1]]);
    }

    #[test]
    fn turning_a_port_off_reads_its_own_mask_once_before_the_write() {
        let ec = Arc::new(Connectors::default());
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.masks_read.lock().unwrap().clear();
        ready(ports.set_enabled(0, false)).unwrap();
        let [own, other] = [laptop_13_pro()[0], laptop_13_pro()[1]];
        assert_eq!(*ec.masks_read.lock().unwrap(), vec![own, other, own]);
    }

    #[test]
    fn a_board_with_no_controller_table_knows_no_mask() {
        let ports = Ports::new(Arc::new(Connectors::default()), Platform::Unknown)
            .expect("the ports still answered");
        let read = ready(ports.ports()).unwrap();
        assert!(
            read.iter()
                .all(|state| matches!(state.mask, PortMask::Unchecked(_)))
        );
    }

    #[test]
    fn a_controller_read_directly_answers_over_a_frozen_entry() {
        let ec = Arc::new(Connectors {
            live: vec![(0, crate::testing::port(1))],
            ..Connectors::default()
        });
        let ports = Ports::new(ec, LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert_eq!(on(&read[0]).partner, PortPartner::Nothing);
        assert_eq!((on(&read[0]).millivolts, on(&read[0]).milliamps), (0, 0));
    }

    #[test]
    fn the_ec_still_answers_which_port_it_charges_from() {
        let ec = Arc::new(Connectors::default());
        let ports = Ports::new(ec, LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert!(on(&read[0]).charging);
    }

    #[test]
    fn a_controller_failing_its_probe_is_read_from_the_ec() {
        let ec = Arc::new(Connectors {
            live: vec![(0, crate::testing::port(1))],
            refusing_controllers: vec![laptop_13_pro()[0]],
            ..Connectors::default()
        });
        let ports = Ports::new(ec, LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert_eq!(on(&read[0]).partner, PortPartner::Source);
    }

    fn mask(ports: &Ports, index: usize) -> PortMask {
        ready(ports.ports()).unwrap()[index].mask.clone()
    }

    #[test]
    fn every_port_whose_mask_reads_is_switchable() {
        let ec = Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        };
        let ports = Ports::new(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let switchable = ready(ports.switchable()).unwrap();
        assert!(switchable.contains(0) && switchable.contains(1));
        assert!(!switchable.contains(2) && !switchable.contains(3));
    }

    #[test]
    fn a_board_with_no_controller_table_switches_nothing() {
        let ports = Ports::new(Arc::new(Connectors::default()), Platform::Unknown)
            .expect("the ports still answered");
        assert!(ready(ports.switchable()).unwrap().is_empty());
        assert!(matches!(
            ready(ports.set_enabled(0, false)),
            Err(DeviceError::InvalidArgs(_))
        ));
    }

    #[test]
    fn turning_a_port_off_writes_only_its_own_bit() {
        let ec = Arc::new(Connectors::default());
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(3, false)).unwrap();
        assert_eq!(
            *ec.masks_written.lock().unwrap(),
            vec![(laptop_13_pro()[1], 0b01)]
        );
        assert_eq!(mask(&ports, 3), PortMask::Off);
        assert!(matches!(mask(&ports, 2), PortMask::On(_)));
    }

    #[test]
    fn turning_a_port_back_on_sets_its_bit() {
        let ec = Arc::new(Connectors::default().with_port_off(LAPTOP_13_PRO, 3));
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(3, true)).unwrap();
        assert_eq!(
            *ec.masks_written.lock().unwrap(),
            vec![(laptop_13_pro()[1], 0b11)]
        );
    }

    #[test]
    fn a_port_already_as_asked_is_not_written() {
        let ec = Arc::new(Connectors::default());
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(0, true)).unwrap();
        assert!(ec.masks_written.lock().unwrap().is_empty());
    }

    #[test]
    fn the_last_port_on_stays_on() {
        let ec = Arc::new(Connectors::default());
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        for port in 0..3 {
            ready(ports.set_enabled(port, false)).unwrap();
        }
        ec.masks_written.lock().unwrap().clear();
        assert!(matches!(
            ready(ports.set_enabled(3, false)),
            Err(DeviceError::InvalidArgs(_))
        ));
        assert!(ec.masks_written.lock().unwrap().is_empty());
        assert!(matches!(mask(&ports, 3), PortMask::On(_)));
    }

    #[test]
    fn a_mask_that_will_not_read_refuses_turning_any_port_off() {
        let ec = Arc::new(Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        assert!(matches!(
            ready(ports.set_enabled(0, false)),
            Err(DeviceError::Failed(_))
        ));
        assert!(ec.masks_written.lock().unwrap().is_empty());
    }

    #[test]
    fn a_mask_that_will_not_read_still_lets_a_port_back_on() {
        let ec = Arc::new(Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default().with_port_off(LAPTOP_13_PRO, 0)
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(0, true)).unwrap();
        assert_eq!(
            *ec.masks_written.lock().unwrap(),
            vec![(laptop_13_pro()[0], 0b11)]
        );
    }

    #[test]
    fn a_controller_driving_one_port_counts_only_that_port() {
        let ec = Arc::new(Connectors {
            controllers: 3,
            count: 5,
            ..Connectors::default()
        });
        let ports =
            Ports::new(ec.clone(), Platform::Laptop16AmdAi300).expect("five ports answered");
        assert!(ready(ports.switchable()).unwrap().contains(4));
        for port in 0..4 {
            ready(ports.set_enabled(port, false)).unwrap();
        }
        assert!(matches!(
            ready(ports.set_enabled(4, false)),
            Err(DeviceError::InvalidArgs(_))
        ));
    }

    #[test]
    fn a_desktop_may_turn_every_port_off() {
        let ec = Arc::new(Connectors {
            controllers: 1,
            count: 2,
            ..Connectors::default()
        });
        let ports =
            Ports::new(ec.clone(), Platform::DesktopAmdAiMax300).expect("two ports answered");
        ready(ports.set_enabled(0, false)).unwrap();
        ready(ports.set_enabled(1, false)).unwrap();
        assert_eq!(mask(&ports, 1), PortMask::Off);
    }

    #[test]
    fn a_port_turned_off_reads_off_once_the_write_returns() {
        let ec = Arc::new(Connectors {
            mask_lag: 3,
            ..Connectors::default()
        });
        let ports = Ports::new(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(1, false)).unwrap();
        assert_eq!(mask(&ports, 1), PortMask::Off);
    }
}
