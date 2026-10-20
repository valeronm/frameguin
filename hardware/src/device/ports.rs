//! The machine's USB-C ports: one device answering for all of them, and the
//! switch each controller's port mask is.

use std::cell::OnceCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_io::Timer;
use async_lock::Mutex;
use frameguin_contract::{
    Board, DeviceError, DeviceResult, OffRefusal, Platform, PortMask, PortReading, PortSet,
    PortState, PortsControl,
};

use crate::drm::{self, Displays, Outputs};
use crate::ec::{Ec, PdPorts};
use crate::pd_controller::{self, Bay, PORTS_PER_CONTROLLER, controller_of};

mod refusal;

/// How long a write waits for the controller's mask to show it.
const SETTLE: Duration = Duration::from_secs(1);
const SETTLE_POLL: Duration = Duration::from_millis(20);

pub struct Ports {
    ec: Arc<dyn PdPorts>,
    outputs: Arc<dyn Outputs>,
    /// How many ports answered at detection. Settled once: the count is the
    /// board's, and re-probing the tail would spend a refused command on
    /// every read to learn what cannot have changed.
    count: u8,
    /// By the EC's controller order, and None for one nothing places.
    controllers: Vec<Option<Controller>>,
    platform: Platform,
    /// A write's settle awaits, and a turn-off weighs every other port's
    /// mask.
    writing: Mutex<()>,
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
        Self::new(ec.clone(), Arc::new(drm::Sysfs), board.platform())
    }

    /// The walk is bounded by the controllers rather than by where the EC
    /// starts refusing, because a board has been seen not to refuse: asked
    /// for a port past its last, one answers success and a reading of
    /// `0xFF`s, having read past its own array. So the count is what the
    /// controllers can account for, and the refusal is only a second bound
    /// under it.
    pub fn new(
        ec: Arc<dyn PdPorts>,
        outputs: Arc<dyn Outputs>,
        platform: Platform,
    ) -> Option<Self> {
        let mut controllers = pd_controller::controllers(platform).to_vec();
        let mut ceiling = ec.pd_controllers().saturating_mul(PORTS_PER_CONTROLLER);
        match pd_controller::bay(platform, ec.as_ref()) {
            Bay::None => {}
            Bay::Controller(controller) => controllers.push(controller),
            Bay::Silent => {
                let mainboard = u8::try_from(controllers.len()).unwrap_or(u8::MAX);
                ceiling = ceiling.min(mainboard.saturating_mul(PORTS_PER_CONTROLLER));
            }
        }
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
            outputs,
            count,
            controllers,
            platform,
            writing: Mutex::new(()),
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

    /// The EC's entry with the controller's registers over it where they
    /// read; a controller that does not answer leaves the port on the entry
    /// for that read. None where the EC refused the entry.
    fn reading(&self, port: u8) -> Option<PortReading> {
        let entry = self.ec.port_state(port).ok().flatten()?;
        Some(match self.controller(port) {
            Some(controller) if controller.direct => {
                match self.ec.port_controller(port, controller.address) {
                    Ok(read) => read.over(&entry),
                    Err(_) => entry,
                }
            }
            _ => entry,
        })
    }

    fn refusal(
        &self,
        masks: &[Option<u8>],
        port: u8,
        video: impl FnOnce() -> Option<bool>,
        displays: impl FnOnce() -> Option<Displays>,
    ) -> Option<OffRefusal> {
        let enabled = (0..self.count).map(|index| (index, enabled_of(masks, index)));
        refusal::off_refusal(self.platform, port, video, displays, enabled)
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
    /// port gone silent costs its own row and not the window.
    async fn ports(&self) -> DeviceResult<Vec<PortState>> {
        let masks = self.masks();
        let displays = OnceCell::new();
        let displays = || *displays.get_or_init(|| self.outputs.displays());
        Ok((0..self.count)
            .filter_map(|index| {
                let enabled = enabled_of(&masks, index);
                if enabled == Some(false) {
                    return Some(PortState {
                        index,
                        mask: PortMask::Off,
                    });
                }
                let reading = self.reading(index)?;
                let refusal = self.refusal(&masks, index, || Some(reading.video), displays);
                let mask = if enabled.is_some() {
                    PortMask::On(reading, refusal)
                } else {
                    PortMask::Unchecked(reading, refusal)
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
        let _writing = self.writing.lock().await;
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
            let video = || self.reading(port).map(|reading| reading.video);
            let displays = || self.outputs.displays();
            if let Some(refusal) = self.refusal(masks, port, video, displays) {
                return Err(refusal::error(refusal));
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
            Timer::after(SETTLE_POLL).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_contract::{
        Cable, CableLatency, CableSpeed, DeviceError, OffRefusal, PdContract, Platform, PortMask,
        PortPartner, PortReading, PortRegisters, PortState, PortsControl, SupplyKind,
    };

    use super::Ports;
    use crate::drm::Displays;
    use crate::pd_controller;
    use crate::testing::{Connectors, ONE_MONITOR, PANEL_ONLY, Screens, ccg8s_module, ready};

    const LAPTOP_13_PRO: Platform = Platform::Laptop13ProUltra3;

    fn detect(ec: Arc<Connectors>, platform: Platform) -> Option<Ports> {
        Ports::new(ec, Arc::new(Screens::default()), platform)
    }

    fn laptop_13_pro() -> &'static [(u8, u16)] {
        pd_controller::controllers(LAPTOP_13_PRO)
    }

    fn on(state: &PortState) -> &PortReading {
        let PortMask::On(reading, _) = &state.mask else {
            panic!("port {} is not on: {:?}", state.index, state.mask);
        };
        reading
    }

    #[test]
    fn the_ports_the_ec_answers_for_are_the_ports_there_are() {
        let ports =
            detect(Arc::new(Connectors::default()), LAPTOP_13_PRO).expect("four ports answered");
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
        assert!(detect(Arc::new(ec), LAPTOP_13_PRO).is_none());
    }

    #[test]
    fn a_board_with_no_pd_controller_has_no_ports() {
        let ec = Connectors {
            controllers: 0,
            ..Connectors::default()
        };
        assert!(detect(Arc::new(ec), LAPTOP_13_PRO).is_none());
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
        let ports = detect(Arc::new(ec), LAPTOP_13_PRO).expect("the ceiling still allows four");
        assert_eq!(ready(ports.ports()).unwrap().len(), 4);
    }

    /// A controller's second port can be absent — the Laptop 16's third
    /// drives one — so the refusal still bounds the walk under the ceiling.
    #[test]
    fn a_controller_driving_one_port_stops_the_walk_under_the_ceiling() {
        let ec = Connectors {
            controllers: 3,
            count: 5,
            bay: Some((0x50, ccg8s_module(0x42))),
            ..Connectors::default()
        };
        let ports = detect(Arc::new(ec), Platform::Laptop16AmdAi300).expect("five ports answered");
        assert_eq!(ready(ports.ports()).unwrap().len(), 5);
    }

    #[test]
    fn a_bay_that_gives_no_descriptor_has_no_port() {
        let ec = Connectors {
            controllers: 3,
            count: 5,
            ..Connectors::default()
        };
        let ports = detect(Arc::new(ec), Platform::Laptop16Amd7040).expect("four ports answered");
        assert_eq!(ready(ports.ports()).unwrap().len(), 4);
        assert!(!ready(ports.switchable()).unwrap().contains(4));
    }

    #[test]
    fn the_bay_controller_is_read_at_the_address_the_module_gives() {
        let ec = Arc::new(Connectors {
            controllers: 3,
            count: 5,
            bay: Some((0x50, ccg8s_module(0x08))),
            ..Connectors::default()
        });
        let ports = detect(ec.clone(), Platform::Laptop16Amd7040).expect("five ports answered");
        ec.controller_reads.lock().unwrap().clear();
        ready(ports.ports()).unwrap();
        assert!(
            ec.controller_reads
                .lock()
                .unwrap()
                .contains(&(4, (5, 0x08)))
        );
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
        let ports = detect(ec, LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert_eq!(on(&read[0]).registers, Some(charging()));
    }

    #[test]
    fn an_empty_port_carries_no_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            ..Connectors::default()
        });
        let ports = detect(ec, LAPTOP_13_PRO).expect("four ports answered");
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
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("the ports still answered");
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
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
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
        let ports = detect(ec.clone(), Platform::Unknown).expect("the ports still answered");
        let read = ready(ports.ports()).unwrap();
        assert!(
            matches!(&read[0].mask, PortMask::Unchecked(reading, _) if reading.registers.is_none())
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
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
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
        let ports = detect(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert!(
            read[..3]
                .iter()
                .all(|state| matches!(state.mask, PortMask::On(..)))
        );
        assert_eq!(read[3].mask, PortMask::Off);
    }

    #[test]
    fn a_stub_turning_one_port_off_leaves_its_neighbour_on() {
        let ec = Connectors::default().with_port_off(LAPTOP_13_PRO, 3);
        let ports = detect(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert!(matches!(read[2].mask, PortMask::On(..)));
        assert_eq!(read[3].mask, PortMask::Off);
    }

    #[test]
    fn a_port_turned_off_is_never_asked_about_its_cable() {
        let ec = Arc::new(Connectors {
            registers: charging(),
            sink: Some(3),
            ..Connectors::default().with_port_off(LAPTOP_13_PRO, 3)
        });
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
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
        let ports = detect(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert!(matches!(read[0].mask, PortMask::On(..)));
        assert!(matches!(read[2].mask, PortMask::Unchecked(..)));
    }

    #[test]
    fn a_mask_refused_at_detection_is_asked_again_on_every_read() {
        let ec = Arc::new(Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        });
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
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
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.masks_read.lock().unwrap().clear();
        assert!(matches!(
            ready(ports.set_enabled(2, true)),
            Err(DeviceError::Failed(_))
        ));
        assert_eq!(*ec.masks_read.lock().unwrap(), vec![laptop_13_pro()[1]]);
    }

    fn video_on(port: u8) -> Arc<Connectors> {
        Arc::new(Connectors {
            video: vec![port],
            ..Connectors::default()
        })
    }

    fn with_screens(ec: &Arc<Connectors>, displays: Option<Displays>) -> (Ports, Arc<Screens>) {
        let screens = Arc::new(Screens {
            displays,
            ..Screens::default()
        });
        let ports = Ports::new(ec.clone(), screens.clone(), LAPTOP_13_PRO).expect("four ports");
        (ports, screens)
    }

    #[test]
    fn the_port_driving_the_only_lit_display_is_refused_turning_off() {
        let ec = video_on(0);
        let (ports, _) = with_screens(&ec, ONE_MONITOR);
        assert!(matches!(
            ready(ports.set_enabled(0, false)),
            Err(DeviceError::InvalidArgs(_))
        ));
        assert!(ec.masks_written.lock().unwrap().is_empty());
    }

    #[test]
    fn a_port_carrying_video_turns_off_beside_the_lit_panel() {
        let ec = video_on(0);
        let (ports, _) = with_screens(&ec, PANEL_ONLY);
        ready(ports.set_enabled(0, false)).unwrap();
        assert_eq!(ec.masks_written.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_port_carrying_video_is_refused_while_the_displays_do_not_read() {
        let ec = video_on(0);
        let (ports, _) = with_screens(&ec, None);
        assert!(matches!(
            ready(ports.set_enabled(0, false)),
            Err(DeviceError::Failed(_))
        ));
        assert!(ec.masks_written.lock().unwrap().is_empty());
    }

    #[test]
    fn a_read_carries_the_refusal_of_the_port_driving_the_only_lit_display() {
        let ec = video_on(0);
        let (ports, screens) = with_screens(&ec, ONE_MONITOR);
        let read = ready(ports.ports()).unwrap();
        assert_eq!(read[0].mask.refusal(), Some(OffRefusal::LastDisplay));
        assert_eq!(read[1].mask.refusal(), None);
        assert_eq!(*screens.reads.lock().unwrap(), 1);
    }

    #[test]
    fn a_read_with_no_port_carrying_video_reads_no_display() {
        let ec = Arc::new(Connectors::default());
        let (ports, screens) = with_screens(&ec, None);
        ready(ports.ports()).unwrap();
        assert_eq!(*screens.reads.lock().unwrap(), 0);
    }

    #[test]
    fn a_read_carries_no_refusal_for_a_port_already_off() {
        let ec = Connectors::default().with_port_off(LAPTOP_13_PRO, 0);
        let ports = detect(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert_eq!(read[0].mask, PortMask::Off);
        assert_eq!(read[0].mask.refusal(), None);
    }

    #[test]
    fn a_port_that_does_not_answer_for_its_state_is_refused_turning_off() {
        let ec = Arc::new(Connectors::default());
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.refusing_states.lock().unwrap().push(0);
        assert!(matches!(
            ready(ports.set_enabled(0, false)),
            Err(DeviceError::Failed(_))
        ));
        assert!(ec.masks_written.lock().unwrap().is_empty());
    }

    #[test]
    fn the_last_port_on_is_refused_though_its_state_does_not_answer() {
        let ec = Arc::new(Connectors::default());
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        for port in 0..3 {
            ready(ports.set_enabled(port, false)).unwrap();
        }
        ec.refusing_states.lock().unwrap().push(3);
        assert!(matches!(
            ready(ports.set_enabled(3, false)),
            Err(DeviceError::InvalidArgs(_))
        ));
    }

    #[test]
    fn a_port_whose_entry_stops_answering_is_left_out_of_the_read() {
        let ec = Arc::new(Connectors::default());
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.refusing_states.lock().unwrap().push(1);
        let read = ready(ports.ports()).unwrap();
        assert_eq!(read.len(), 3);
        assert!(read.iter().all(|state| state.index != 1));
    }

    #[test]
    fn a_port_without_video_turns_off_without_reading_the_displays() {
        let ec = Arc::new(Connectors::default());
        let (ports, screens) = with_screens(&ec, None);
        ready(ports.set_enabled(0, false)).unwrap();
        assert_eq!(*screens.reads.lock().unwrap(), 0);
    }

    #[test]
    fn turning_a_port_off_reads_its_own_mask_once_before_the_write() {
        let ec = Arc::new(Connectors::default());
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ec.masks_read.lock().unwrap().clear();
        ready(ports.set_enabled(0, false)).unwrap();
        let [own, other] = [laptop_13_pro()[0], laptop_13_pro()[1]];
        assert_eq!(*ec.masks_read.lock().unwrap(), vec![own, other, own]);
    }

    #[test]
    fn a_board_with_no_controller_table_knows_no_mask() {
        let ports = detect(Arc::new(Connectors::default()), Platform::Unknown)
            .expect("the ports still answered");
        let read = ready(ports.ports()).unwrap();
        assert!(
            read.iter()
                .all(|state| matches!(state.mask, PortMask::Unchecked(..)))
        );
    }

    #[test]
    fn a_controller_read_directly_answers_over_a_frozen_entry() {
        let ec = Arc::new(Connectors {
            live: vec![(0, crate::testing::port(1))],
            ..Connectors::default()
        });
        let ports = detect(ec, LAPTOP_13_PRO).expect("four ports answered");
        let read = ready(ports.ports()).unwrap();
        assert_eq!(on(&read[0]).partner, PortPartner::Nothing);
        assert_eq!((on(&read[0]).millivolts, on(&read[0]).milliamps), (0, 0));
    }

    #[test]
    fn the_ec_still_answers_which_port_it_charges_from() {
        let ec = Arc::new(Connectors::default());
        let ports = detect(ec, LAPTOP_13_PRO).expect("four ports answered");
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
        let ports = detect(ec, LAPTOP_13_PRO).expect("four ports answered");
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
        let ports = detect(Arc::new(ec), LAPTOP_13_PRO).expect("four ports answered");
        let switchable = ready(ports.switchable()).unwrap();
        assert!(switchable.contains(0) && switchable.contains(1));
        assert!(!switchable.contains(2) && !switchable.contains(3));
    }

    #[test]
    fn a_board_with_no_controller_table_switches_nothing() {
        let ports = detect(Arc::new(Connectors::default()), Platform::Unknown)
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
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(3, false)).unwrap();
        assert_eq!(
            *ec.masks_written.lock().unwrap(),
            vec![(laptop_13_pro()[1], 0b01)]
        );
        assert_eq!(mask(&ports, 3), PortMask::Off);
        assert!(matches!(mask(&ports, 2), PortMask::On(..)));
    }

    #[test]
    fn turning_a_port_back_on_sets_its_bit() {
        let ec = Arc::new(Connectors::default().with_port_off(LAPTOP_13_PRO, 3));
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(3, true)).unwrap();
        assert_eq!(
            *ec.masks_written.lock().unwrap(),
            vec![(laptop_13_pro()[1], 0b11)]
        );
    }

    #[test]
    fn a_port_already_as_asked_is_not_written() {
        let ec = Arc::new(Connectors::default());
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(0, true)).unwrap();
        assert!(ec.masks_written.lock().unwrap().is_empty());
    }

    #[test]
    fn the_last_port_on_stays_on() {
        let ec = Arc::new(Connectors::default());
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        for port in 0..3 {
            ready(ports.set_enabled(port, false)).unwrap();
        }
        ec.masks_written.lock().unwrap().clear();
        assert!(matches!(
            ready(ports.set_enabled(3, false)),
            Err(DeviceError::InvalidArgs(_))
        ));
        assert!(ec.masks_written.lock().unwrap().is_empty());
        assert!(matches!(mask(&ports, 3), PortMask::On(..)));
    }

    #[test]
    fn a_mask_that_will_not_read_refuses_only_the_last_port_known_on() {
        let ec = Arc::new(Connectors {
            refusing_masks: vec![laptop_13_pro()[1]],
            ..Connectors::default()
        });
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        ready(ports.set_enabled(0, false)).unwrap();
        ec.masks_written.lock().unwrap().clear();
        assert!(matches!(
            ready(ports.set_enabled(1, false)),
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
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
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
            bay: Some((0x50, ccg8s_module(0x42))),
            ..Connectors::default()
        });
        let ports = detect(ec.clone(), Platform::Laptop16AmdAi300).expect("five ports answered");
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
        let ports = detect(ec.clone(), Platform::DesktopAmdAiMax300).expect("two ports answered");
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
        let ports = detect(ec.clone(), LAPTOP_13_PRO).expect("four ports answered");
        async_io::block_on(ports.set_enabled(1, false)).unwrap();
        assert_eq!(mask(&ports, 1), PortMask::Off);
    }

    #[test]
    fn two_ports_on_one_controller_turned_off_at_once_both_read_off() {
        let ec = Arc::new(Connectors {
            mask_lag: 3,
            ..Connectors::default()
        });
        let ports = detect(ec, LAPTOP_13_PRO).expect("four ports answered");
        let (first, second) = async_io::block_on(futures_lite::future::zip(
            ports.set_enabled(0, false),
            ports.set_enabled(1, false),
        ));
        first.unwrap();
        second.unwrap();
        assert_eq!(mask(&ports, 0), PortMask::Off);
        assert_eq!(mask(&ports, 1), PortMask::Off);
    }
}
