//! The devices, one module each: what detects it, the mirror it keeps, and
//! the facets it offers — its control trait from `frameguin_wire`, its
//! [`crate::part::Part`], or both.
//!
//! A device holds only the roles it needs — a `dyn` transport, and a
//! [`crate::mirror::Mirror`] for what it cannot read back — so its logic
//! runs against stubs in tests, and nothing of the bus: authorization is the bus's business, and a caller
//! linking this crate directly has already got past it.

pub mod battery;
pub(crate) mod display;
pub(crate) mod mainboard;
pub(crate) mod memory;
pub mod ports;
pub mod power_led;
pub(crate) mod storage;
pub mod touchpad;
pub mod touchscreen;

use std::sync::Arc;

use crate::device::battery::Battery;
use crate::device::display::Display;
use crate::device::mainboard::Mainboard;
use crate::device::memory::Module;
use crate::device::ports::Ports;
use crate::device::power_led::PowerLed;
use crate::device::storage::Drive;
use crate::device::touchpad::Touchpad;
use crate::device::touchscreen::Touchscreen;
use crate::ec::Ec;
use crate::lifetime::{self, Holders};
use crate::mirror::Mirrors;
use crate::part::{Identity, Part};
use crate::restore::Restore;
use crate::state::{StateFile, Store};

/// Every device that is a control, None where detection found none.
pub struct Devices {
    pub battery: Option<Battery>,
    pub touchpad: Option<Touchpad>,
    pub touchscreen: Option<Touchscreen>,
    pub power_led: Option<PowerLed>,
    pub ports: Option<Ports>,
}

pub struct Detected {
    pub devices: Devices,
    /// Every part found, whether or not it is also a control.
    pub parts: Vec<Identity>,
    pub restore: Restore,
}

/// Opens every transport: the devices that are controls, the parts, and the
/// switch their mirrors share.
pub fn detect() -> Detected {
    let store: Arc<dyn Store> = Arc::new(StateFile::load());
    let ec = Ec::open().map(Arc::new);
    let holders = Holders::new(
        ec.as_ref().and_then(|ec| ec.boot().ok()),
        lifetime::host_boot(),
    );
    let mirrors = Mirrors::new(store, holders);
    // One walk of the HID bus for every device asked about: building an
    // `HidApi` enumerates the lot.
    let hid = hidapi::HidApi::new().ok();
    let touchpad = hid.as_ref().and_then(|hid| Touchpad::detect(hid, &mirrors));
    let (touchscreen, controller_firmware) = hid
        .as_ref()
        .map_or((None, None), |hid| Touchscreen::detect(hid, &mirrors));
    let power_led = ec.as_ref().and_then(|ec| PowerLed::detect(ec, &mirrors));
    let battery = ec.as_ref().and_then(|ec| Battery::detect(ec, &mirrors));
    let mainboard = Mainboard::detect(ec.as_deref());
    let memory = Module::detect();
    let drives = Drive::detect();
    let displays = Display::detect(controller_firmware);
    let parts = mainboard
        .iter()
        .map(Part::identity)
        .chain(battery.iter().map(Part::identity))
        .chain(touchpad.iter().map(Part::identity))
        .chain(memory.iter().map(Part::identity))
        .chain(drives.iter().map(Part::identity))
        .chain(displays.iter().map(Part::identity))
        .cloned()
        .collect();
    Detected {
        devices: Devices {
            battery,
            touchpad,
            touchscreen,
            power_led,
            ports: ec.as_ref().and_then(Ports::detect),
        },
        parts,
        restore: mirrors.restore(),
    }
}
