//! The haptic touchpad's transport: `framework_lib` drives it over the pad's
//! own HID reports rather than through the EC.
//!
//! What the device needs of it is [`HapticPad`]; [`Hid`] is the pad itself.
//! Every write is write-only — the firmware ACKs `GET_FEATURE` with zeros —
//! so what was set is only knowable from the mirror [`crate::device::touchpad`]
//! keeps.

use frameguin_wire::{self as wire, DeviceError, DeviceResult};
use framework_lib::touchpad::{self, ClickForce};

/// Known haptic touchpad models (`PixArt` PIDs). A curated device list, per
/// the probe rule: the haptic setters have no side-effect-free probe
/// (they're write-only, and every PTP touchpad accepts the open — only
/// haptic ones act on the reports). Keying on the touchpad's own HID
/// identity rather than the board name means a haptic pad retrofitted into
/// an older laptop is recognized. Extend when Framework ships new haptic
/// pads.
const HAPTIC_PIDS: [u16; 1] = [0x1343];

/// What the pad ships with, and so what a read before any write answers
/// with, in the wire's terms since the getter is what answers it.
pub const DEFAULT_CLICK_FORCE: wire::ClickForce = wire::ClickForce::Medium;

/// The writes the haptic touchpad takes, in the wire's vocabulary so that a
/// device over it needs none of the transport's.
pub trait HapticPad: Send + Sync {
    fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()>;
    fn set_click_force(&self, force: wire::ClickForce) -> DeviceResult<()>;
}

/// The pad on the HID bus.
pub struct Hid;

impl HapticPad for Hid {
    fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()> {
        touchpad::set_haptic_intensity(percent).map_err(device_error)
    }

    fn set_click_force(&self, force: wire::ClickForce) -> DeviceResult<()> {
        touchpad::set_click_force(click_force(force)).map_err(device_error)
    }
}

fn device_error(error: impl std::fmt::Display) -> DeviceError {
    DeviceError::Failed(error.to_string())
}

/// The haptic pad on the bus, if there is one, as the bus describes it.
///
/// Takes the enumeration rather than making one: `HidApi::new` walks every
/// HID device on the machine before a caller asks it anything, and the
/// daemon has more than one device to look for.
pub fn haptic_pad(hid: &hidapi::HidApi) -> Option<&hidapi::DeviceInfo> {
    hid.device_list()
        .find(|dev| dev.vendor_id() == touchpad::PIX_VID && HAPTIC_PIDS.contains(&dev.product_id()))
}

pub fn click_force(force: wire::ClickForce) -> ClickForce {
    match force {
        wire::ClickForce::Low => ClickForce::Low,
        wire::ClickForce::Medium => ClickForce::Medium,
        wire::ClickForce::High => ClickForce::High,
    }
}

/// The device code the state file carries, back to the wire's name; None for
/// a code no force maps to.
pub fn wire_click_force(code: u8) -> Option<wire::ClickForce> {
    wire::ClickForce::ALL
        .into_iter()
        .find(|force| click_force(*force) as u8 == code)
}

#[cfg(test)]
mod tests {
    /// The app offers these steps but cannot link `framework_lib` to learn
    /// them, so `wire` carries the list and this is what keeps the copy
    /// honest. A firmware generation that changes the steps should fail here
    /// rather than in a combo that silently offers the wrong ones.
    #[test]
    fn the_wire_haptic_steps_are_the_ones_the_touchpad_implements() {
        assert_eq!(
            frameguin_wire::HAPTIC_INTENSITY_LEVELS,
            framework_lib::touchpad::HAPTIC_INTENSITY_LEVELS
        );
    }

    /// The store carries the device's own code, so the default has to map
    /// to one and back.
    #[test]
    fn the_stored_default_is_the_force_the_getter_names() {
        assert_eq!(
            super::wire_click_force(super::click_force(super::DEFAULT_CLICK_FORCE) as u8),
            Some(super::DEFAULT_CLICK_FORCE)
        );
    }

    /// The state file stores these codes, so they outlive the process that
    /// wrote them. They are the HID protocol's own numbering rather than
    /// declaration order, which is what makes them safe to persist — pinned
    /// here so a `framework_lib` that renumbered them could not silently
    /// reinterpret every saved file as a different force.
    #[test]
    fn the_stored_codes_are_the_ones_the_hid_report_carries() {
        use super::ClickForce;
        assert_eq!(
            [
                ClickForce::Low as u8,
                ClickForce::Medium as u8,
                ClickForce::High as u8
            ],
            [1, 2, 3]
        );
    }
}
