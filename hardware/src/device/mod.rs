//! The devices, one module each: what detects it, the mirror it keeps, and
//! the facets it offers — its control trait from `frameguin_wire`, its
//! [`crate::part::Part`], or both.
//!
//! A device holds only the roles it needs — a `dyn` transport, and a
//! `Mirror` for what it cannot read back — so its logic runs against stubs
//! in tests, and nothing of the bus: authorization is the bus's business,
//! and a caller linking this crate directly has already got past it.

pub mod battery;
pub(crate) mod camera;
pub mod charging_led;
pub mod chassis;
pub(crate) mod display;
pub(crate) mod fingerprint;
pub(crate) mod mainboard;
pub(crate) mod memory;
pub mod ports;
pub mod power_led;
pub mod privacy_switches;
pub(crate) mod storage;
pub mod touchpad;
pub mod touchscreen;
pub mod usb;
pub(crate) mod wifi;

use std::sync::Arc;

use frameguin_wire::Board;

use crate::device::battery::Battery;
use crate::device::camera::Camera;
use crate::device::charging_led::ChargingLed;
use crate::device::chassis::Chassis;
use crate::device::display::Display;
use crate::device::fingerprint::Fingerprint;
use crate::device::mainboard::Mainboard;
use crate::device::memory::Module;
use crate::device::ports::Ports;
use crate::device::power_led::PowerLed;
use crate::device::privacy_switches::PrivacySwitches;
use crate::device::storage::Drive;
use crate::device::touchpad::Touchpad;
use crate::device::touchscreen::Touchscreen;
use crate::device::usb::Usb;
use crate::device::wifi::Wifi;
use crate::dmi;
use crate::ec::Ec;
use crate::lifetime::{self, Holders};
use crate::mirror::Mirrors;
use crate::part::{Identity, Part};
use crate::restore::Restore;
use crate::state::{StateFile, Store};
use crate::usb::devices as bus_devices;

/// Every device that is a control, None where detection found none.
pub struct Devices {
    pub battery: Option<Battery>,
    pub touchpad: Option<Touchpad>,
    pub touchscreen: Option<Touchscreen>,
    pub power_led: Option<PowerLed>,
    pub charging_led: Option<ChargingLed>,
    pub ports: Option<Ports>,
    pub chassis: Option<Chassis>,
    pub privacy_switches: Option<PrivacySwitches>,
    pub usb: Option<Usb>,
}

pub struct Detected {
    pub board: Board,
    pub devices: Devices,
    /// Every part found, whether or not it is also a control.
    pub parts: Vec<Identity>,
    pub restore: Restore,
}

/// Opens every transport: the board, the devices that are controls, the
/// parts, and the switch their mirrors share.
pub fn detect() -> Detected {
    let board = dmi::board();
    let store: Arc<dyn Store> = Arc::new(StateFile::load());
    let ec = Ec::open(&board).map(Arc::new);
    let holders = Holders::new(
        ec.as_ref().and_then(|ec| ec.boot().ok()),
        lifetime::host_boot(),
    );
    let mirrors = Mirrors::new(store, holders);
    // One walk of the HID bus for every device asked about: building an
    // `HidApi` enumerates the lot.
    let hid = hidapi::HidApi::new().ok();
    let touchpad = hid.as_ref().and_then(|hid| Touchpad::detect(hid, &mirrors));
    let (touchscreen, controller_firmware) = hid.as_ref().map_or((None, None), |hid| {
        Touchscreen::detect(hid, &board, &mirrors)
    });
    let power_led = ec.as_ref().and_then(|ec| PowerLed::detect(ec, &mirrors));
    let battery = ec.as_ref().and_then(|ec| Battery::detect(ec, &mirrors));
    let mainboard = Mainboard::detect(&board, ec.as_deref());
    let memory = Module::detect();
    let drives = Drive::detect();
    let radios = Wifi::detect();
    let displays = Display::detect(controller_firmware);
    // One walk of the USB bus for every part read off it, as the HID bus
    // above.
    let bus = bus_devices();
    let camera = Camera::detect(&bus);
    let fingerprint = Fingerprint::detect(&bus);
    let parts = mainboard
        .iter()
        .map(Part::identity)
        .chain(battery.iter().map(Part::identity))
        .chain(touchpad.iter().map(Part::identity))
        .chain(memory.iter().map(Part::identity))
        .chain(drives.iter().map(Part::identity))
        .chain(radios.iter().map(Part::identity))
        .chain(displays.iter().map(Part::identity))
        .chain(camera.iter().map(Part::identity))
        .chain(fingerprint.iter().map(Part::identity))
        .cloned()
        .collect();
    Detected {
        devices: Devices {
            battery,
            touchpad,
            touchscreen,
            power_led,
            charging_led: ec.as_ref().and_then(ChargingLed::detect),
            ports: ec.as_ref().and_then(Ports::detect),
            chassis: ec.as_ref().and_then(Chassis::detect),
            privacy_switches: ec.as_ref().and_then(PrivacySwitches::detect),
            usb: Usb::detect(&board),
        },
        parts,
        restore: mirrors.restore(),
        board,
    }
}
