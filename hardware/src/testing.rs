//! Stubs for the roles and the store, so a device's logic runs against them
//! in this crate's tests and in another crate's. A stub's `Default` is the
//! machine that takes every write and answers every read; each knob names
//! one way of not doing so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use frameguin_wire::{
    Attached, BatteryCondition, BatteryInfo, BatteryState, CcPolarity, ChargeFlow, ChassisState,
    ClickForce, DataRole, DeckState, Detail, DeviceError, DeviceResult, Epr, ExtenderStage,
    ExtenderState, Identity, LinkState, NetworkLink, PartKind, PortPartner, PortState,
    PowerLedLevel, PowerRole, PrivacyState, UsbSpeed,
};

use crate::ec::{Charger, ChassisEc, Pack, PdPorts, PowerLedEc, PrivacyEc};
use crate::led::LedClass;
use crate::lifetime::{EcBoot, Holders};
use crate::mirror::Mirrors;
use crate::part;
use crate::state::{self, Store};
use crate::touchpad::HapticPad;
use crate::touchscreen::TouchSwitch;
use crate::usb::{RootDevice, UsbTree};

/// One PD controller's version blob, as a controller really answered: base
/// `3.8.50.00A`, application `1.0.0A`.
pub const PD_VERSION: [u8; crate::pd::VERSION_LEN] =
    [0x0a, 0x00, 0x50, 0x38, 0x62, 0x6e, 0x0a, 0x10];

/// An EC boot a mirror's evidence can name and a later reading can match.
pub const EC_BOOT: EcBoot = EcBoot::from_clocks(500_000, 1_000_000);

/// The EC an hour after [`EC_BOOT`], having restarted a minute ago.
pub const EC_RESTARTED: EcBoot = EcBoot::from_clocks(60, 1_003_600);

/// The host's boot id, and the id of the boot before it.
pub const HOST_BOOT: &str = "00000000-0000-4000-8000-000000000001";
pub const HOST_EARLIER: &str = "00000000-0000-4000-8000-000000000002";

/// The haptic touchpad as detection would identify it, the descriptor
/// naming no maker and the vendor list naming one.
pub fn touchpad_identity() -> Identity {
    part::hid(
        PartKind::Touchpad,
        0x093a,
        0x1343,
        "Pixart Imaging, Inc.",
        "Haptic touchpad",
        "",
    )
}

/// The panel as its EDID identifies it, with the maker the vendor list
/// resolved.
pub fn display_identity() -> Identity {
    Identity {
        details: vec![
            Detail::Resolution {
                across: 2880,
                down: 1920,
            },
            Detail::PanelSize {
                across: 285,
                down: 190,
            },
            Detail::ColourDepth(10),
            Detail::RefreshRate {
                slowest: 30,
                fastest: 120,
            },
            Detail::ManufactureYear(2025),
        ],
        ..part::edid(
            "CSW",
            "China Star Optoelectronics Technology Co., Ltd",
            4898,
            "MND508ZB1-1",
            "",
        )
    }
}

/// The pack as its own registers identify it.
pub fn battery_identity() -> Identity {
    part::sbs(
        "NVT",
        "FRANGWA",
        "0001",
        3_900,
        15_400,
        Some("2026-01-01".to_owned()),
    )
}

/// A store that never touches disk.
#[derive(Default)]
pub struct Memory(Mutex<BTreeMap<String, String>>);

impl Store for Memory {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }

    fn set(&self, key: &str, value: Option<String>) {
        state::apply(&mut self.0.lock().unwrap(), key, value);
    }

    fn drop_prefix(&self, prefix: &str) {
        state::drop_prefix(&mut self.0.lock().unwrap(), prefix);
    }
}

/// Mirrors over a store in memory, on a machine whose holders answer as
/// named.
pub fn mirrors(store: &Arc<Memory>, ec: Option<EcBoot>, host: Option<&str>) -> Mirrors {
    Mirrors::new(store.clone(), Holders::new(ec, host.map(str::to_owned)))
}

/// Polls once: the direct implementation never pends.
pub fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("the direct implementation never pends"),
    }
}

/// The block [`Gauge`] answers with.
pub fn block() -> BatteryInfo {
    BatteryInfo {
        state: BatteryState {
            percent: 80,
            flow: ChargeFlow::Idle,
            milliamps: 0,
            millivolts: 15_000,
        },
        remaining_capacity: 3_000,
        last_full_capacity: 3_600,
        design_capacity: 3_900,
        design_millivolts: 15_400,
        cycle_count: 12,
        charger_connected: true,
        critical: false,
    }
}

/// A pack that answers its block, and its condition unless told not to.
pub struct Gauge {
    pub answering: bool,
}

impl Default for Gauge {
    fn default() -> Self {
        Self { answering: true }
    }
}

impl Pack for Gauge {
    fn identity(&self) -> Option<Identity> {
        Some(battery_identity())
    }

    fn info(&self) -> Option<BatteryInfo> {
        Some(block())
    }

    fn condition(&self) -> Option<BatteryCondition> {
        self.answering.then(|| BatteryCondition {
            cell_millivolts: vec![3_750, 3_751, 3_749, 3_750],
            alarms: Vec::new(),
            decicelsius: 301,
        })
    }
}

/// An extender two days into its countdown.
pub const EXTENDER: ExtenderState = ExtenderState {
    enabled: true,
    stage: ExtenderStage::Inactive,
    trigger_days: 5,
    first_stage_seconds: 3 * 86_400,
    reset_minutes: 30,
};

/// A charger holding one ceiling, taking every cap unless told to refuse
/// them, and logging what it took.
pub struct EcCharger {
    pub limit: Mutex<u8>,
    pub caps: bool,
    pub refusing: bool,
    pub written: Mutex<Vec<u32>>,
    pub extender: bool,
}

impl Default for EcCharger {
    fn default() -> Self {
        Self {
            limit: Mutex::new(100),
            caps: true,
            refusing: false,
            written: Mutex::default(),
            extender: true,
        }
    }
}

impl Charger for EcCharger {
    fn charge_limit(&self) -> DeviceResult<u8> {
        Ok(*self.limit.lock().unwrap())
    }

    fn set_charge_limit(&self, percent: u8) -> DeviceResult<()> {
        *self.limit.lock().unwrap() = percent;
        Ok(())
    }

    fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<()> {
        if self.refusing {
            return Err(DeviceError::Failed("no EC".into()));
        }
        self.written.lock().unwrap().push(milliamps);
        Ok(())
    }

    fn charge_current_limit_supported(&self) -> bool {
        self.caps
    }

    fn extender(&self) -> DeviceResult<ExtenderState> {
        if self.extender {
            Ok(EXTENDER)
        } else {
            Err(DeviceError::Failed("invalid command".into()))
        }
    }
}

/// Every write the EC and the kernel took, in the order they took them.
pub type Log = Arc<Mutex<Vec<String>>>;

/// An EC holding one level, logging every write, and refusing them all
/// once told to.
pub struct LedEc {
    pub level: Mutex<(u8, PowerLedLevel)>,
    pub custom: bool,
    pub refusing: bool,
    pub log: Log,
}

impl Default for LedEc {
    fn default() -> Self {
        Self {
            level: Mutex::new((55, PowerLedLevel::High)),
            custom: true,
            refusing: false,
            log: Log::default(),
        }
    }
}

impl PowerLedEc for LedEc {
    fn power_led_level(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        Ok(*self.level.lock().unwrap())
    }

    fn set_power_led_level(&self, level: PowerLedLevel) -> DeviceResult<()> {
        if self.refusing {
            return Err(DeviceError::Failed("no EC".into()));
        }
        self.level.lock().unwrap().1 = level;
        self.log.lock().unwrap().push(format!("level {level:?}"));
        Ok(())
    }

    fn set_power_led_percentage(&self, percent: u8) -> DeviceResult<()> {
        if self.refusing {
            return Err(DeviceError::Failed("no EC".into()));
        }
        *self.level.lock().unwrap() = (percent, PowerLedLevel::Custom);
        self.log.lock().unwrap().push(format!("percent {percent}"));
        Ok(())
    }

    fn custom_power_led_levels(&self) -> bool {
        self.custom
    }
}

/// A LED class with one node, or none, keeping the kernel's account of
/// whether the LED is held dark.
pub struct Leds {
    pub node: Option<PathBuf>,
    pub dark: Mutex<bool>,
    pub refusing_release: bool,
    pub log: Log,
}

impl Default for Leds {
    fn default() -> Self {
        Self {
            node: Some(PathBuf::from("/sys/class/leds/power")),
            dark: Mutex::new(false),
            refusing_release: false,
            log: Log::default(),
        }
    }
}

impl LedClass for Leds {
    fn controllable(&self) -> Option<PathBuf> {
        self.node.clone()
    }

    fn held_dark(&self) -> Option<PathBuf> {
        self.dark
            .lock()
            .unwrap()
            .then(|| self.node.clone())
            .flatten()
    }

    fn darken(&self, _dir: &Path) -> DeviceResult<()> {
        *self.dark.lock().unwrap() = true;
        self.log.lock().unwrap().push("darken".into());
        Ok(())
    }

    fn release(&self, _dir: &Path) -> DeviceResult<()> {
        if self.refusing_release {
            return Err(DeviceError::Failed("trigger: permission denied".into()));
        }
        *self.dark.lock().unwrap() = false;
        self.log.lock().unwrap().push("release".into());
        Ok(())
    }
}

/// A pad that takes every write, or refuses every one.
#[derive(Default)]
pub struct Haptic {
    pub refusing: bool,
}

impl Haptic {
    fn answer(&self) -> DeviceResult<()> {
        if self.refusing {
            Err(DeviceError::Failed("no pad".into()))
        } else {
            Ok(())
        }
    }
}

impl HapticPad for Haptic {
    fn set_haptic_intensity(&self, _percent: u8) -> DeviceResult<()> {
        self.answer()
    }

    fn set_click_force(&self, _force: ClickForce) -> DeviceResult<()> {
        self.answer()
    }
}

/// One port as a stub answers for it: the first is charging under a 100 W
/// contract, every other is empty.
pub fn port(index: u8) -> PortState {
    let charging = index == 0;
    PortState {
        index,
        partner: if charging {
            PortPartner::Source
        } else {
            PortPartner::Nothing
        },
        contract: charging,
        power_role: PowerRole::Sink,
        data_role: if charging {
            DataRole::DownstreamFacing
        } else {
            DataRole::UpstreamFacing
        },
        millivolts: if charging { 20_000 } else { 0 },
        milliamps: if charging { 5000 } else { 0 },
        charging,
        video: false,
        vconn: charging,
        cc: CcPolarity::Cc1,
        epr: Epr::Unsupported,
    }
}

/// An EC answering for two controllers and `count` ports, refusing every
/// number past them — or answering past them anyway, the way a board has
/// been seen to.
pub struct Connectors {
    pub controllers: u8,
    pub count: u8,
    /// Answers for any port asked for rather than refusing one past the
    /// last, which is what a board does that reads past its own array.
    pub refusing_none: bool,
}

impl Default for Connectors {
    fn default() -> Self {
        Self {
            controllers: 2,
            count: 4,
            refusing_none: false,
        }
    }
}

impl PdPorts for Connectors {
    fn pd_controllers(&self) -> u8 {
        self.controllers
    }

    fn port_state(&self, index: u8) -> DeviceResult<Option<PortState>> {
        Ok((self.refusing_none || index < self.count).then(|| port(index)))
    }
}

pub struct Cover {
    pub state: ChassisState,
    /// Firmware without the chassis commands.
    pub refusing: bool,
    /// None for firmware without the deck state command.
    pub deck: Option<DeckState>,
}

impl Default for Cover {
    fn default() -> Self {
        Self {
            state: ChassisState {
                open: false,
                opened: 2,
                found_open: 1,
            },
            refusing: false,
            deck: Some(DeckState::On),
        }
    }
}

impl ChassisEc for Cover {
    fn chassis(&self) -> DeviceResult<ChassisState> {
        if self.refusing {
            return Err(DeviceError::Failed("invalid command".into()));
        }
        Ok(self.state)
    }

    fn deck_state(&self) -> DeviceResult<DeckState> {
        self.deck
            .ok_or_else(|| DeviceError::Failed("invalid command".into()))
    }
}

pub struct Sliders {
    pub switches: PrivacyState,
    /// Firmware without the privacy switches command.
    pub refusing: bool,
}

impl Default for Sliders {
    fn default() -> Self {
        Self {
            switches: PrivacyState {
                camera: true,
                microphone: false,
            },
            refusing: false,
        }
    }
}

impl PrivacyEc for Sliders {
    fn privacy_switches(&self) -> DeviceResult<PrivacyState> {
        if self.refusing {
            return Err(DeviceError::Failed("invalid command".into()));
        }
        Ok(self.switches)
    }
}

/// A route holding a level it reports — the pad's, on by default — or
/// holding nothing, and failing every read or refusing every write once
/// told to.
pub struct Route {
    pub level: Mutex<Option<bool>>,
    pub unreadable: bool,
    pub refusing: bool,
}

impl Default for Route {
    fn default() -> Self {
        Self {
            level: Mutex::new(Some(true)),
            unreadable: false,
            refusing: false,
        }
    }
}

impl TouchSwitch for Route {
    fn reading(&self) -> DeviceResult<Option<bool>> {
        if self.unreadable {
            return Err(DeviceError::Failed("no line".into()));
        }
        Ok(*self.level.lock().unwrap())
    }

    fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        if self.refusing {
            return Err(DeviceError::Failed("no panel".into()));
        }
        if let Some(level) = self.level.lock().unwrap().as_mut() {
            *level = enabled;
        }
        Ok(())
    }
}

pub struct Hub {
    pub devices: Vec<Attached>,
    /// A machine whose USB bus cannot be listed.
    pub refusing: bool,
    /// What every card's firmware report answers.
    pub firmware: Option<String>,
    /// The device directories firmware was asked of.
    pub asked: Mutex<Vec<PathBuf>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            devices: vec![
                Attached {
                    controller: "0000:00:0d.0".to_owned(),
                    root_port: 2,
                    vendor_id: 0x04c5,
                    product_id: 0x2028,
                    manufacturer: "iODD".to_owned(),
                    product: "iodd_ST400".to_owned(),
                    speed: UsbSpeed::Super,
                    firmware: String::new(),
                    network: Vec::new(),
                },
                Attached {
                    controller: "0000:00:0d.0".to_owned(),
                    root_port: 3,
                    vendor_id: 0x0bda,
                    product_id: 0x8153,
                    manufacturer: "Realtek".to_owned(),
                    product: "USB 10/100/1000 LAN".to_owned(),
                    speed: UsbSpeed::Super,
                    firmware: String::new(),
                    network: vec![NetworkLink {
                        interface: "enp0s13f0u3".to_owned(),
                        mac: "00:e0:4c:68:02:00".to_owned(),
                        state: LinkState::Up,
                        megabits: 1000,
                    }],
                },
                Attached {
                    controller: "0000:00:14.0".to_owned(),
                    root_port: 5,
                    vendor_id: 0x32ac,
                    product_id: 0x0002,
                    manufacturer: "Framework".to_owned(),
                    product: "HDMI Expansion Card".to_owned(),
                    speed: UsbSpeed::Full,
                    firmware: "3.0.10.06A".to_owned(),
                    network: Vec::new(),
                },
            ],
            refusing: false,
            firmware: Some("3.0.10.06A".to_owned()),
            asked: Mutex::default(),
        }
    }
}

impl UsbTree for Hub {
    fn root_devices(&self) -> DeviceResult<Vec<RootDevice>> {
        if self.refusing {
            return Err(DeviceError::Failed("no USB bus".into()));
        }
        Ok(self
            .devices
            .iter()
            .map(|attached| RootDevice {
                attached: Attached {
                    firmware: String::new(),
                    network: Vec::new(),
                    ..attached.clone()
                },
                path: hub_path(attached),
            })
            .collect())
    }

    fn card_firmware(&self, device: &Path) -> Option<String> {
        self.asked.lock().unwrap().push(device.to_owned());
        self.firmware.clone()
    }

    fn network(&self, device: &Path) -> Vec<NetworkLink> {
        self.devices
            .iter()
            .find(|attached| hub_path(attached) == device)
            .map(|attached| attached.network.clone())
            .unwrap_or_default()
    }
}

fn hub_path(attached: &Attached) -> PathBuf {
    PathBuf::from(format!("{}-{}", attached.controller, attached.root_port))
}
