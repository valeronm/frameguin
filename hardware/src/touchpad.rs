//! The haptic touchpad's transport, over the pad's own HID reports rather
//! than through the EC: `framework_lib` makes the writes, and the firmware
//! version is a register read made here.
//!
//! What the device needs of it is [`HapticPad`]; [`Hid`] is the pad itself.
//! Both settings are write-only: the pad answers `GET_FEATURE` on their
//! reports with an empty report.

use frameguin_contract::{self as contract, DeviceError, DeviceResult};
use framework_lib::touchpad::{self, ClickForce};

/// Known haptic touchpad models (`PixArt` PIDs). A curated device list, per
/// the probe rule: the haptic setters have no side-effect-free probe
/// (they're write-only, and every PTP touchpad accepts the open — only
/// haptic ones act on the reports). Keying on the touchpad's own HID
/// identity rather than the board name means a haptic pad retrofitted into
/// an older laptop is recognized. Extend when Framework ships new haptic
/// pads.
const HAPTIC_PIDS: [u16; 1] = [0x1343];

/// What the pad ships with, there being no readback to ask instead.
pub(crate) const DEFAULT_CLICK_FORCE: contract::ClickForce = contract::ClickForce::Medium;

/// The writes the haptic touchpad takes, in the contract's vocabulary so that a
/// device over it needs none of the transport's.
pub trait HapticPad: Send + Sync {
    fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()>;
    fn set_click_force(&self, force: contract::ClickForce) -> DeviceResult<()>;
}

/// The pad on the HID bus.
pub(crate) struct Hid;

impl HapticPad for Hid {
    fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()> {
        touchpad::set_haptic_intensity(percent).map_err(device_error)
    }

    fn set_click_force(&self, force: contract::ClickForce) -> DeviceResult<()> {
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
pub(crate) fn haptic_pad(hid: &hidapi::HidApi) -> Option<&hidapi::DeviceInfo> {
    hid.device_list()
        .find(|dev| dev.vendor_id() == touchpad::PIX_VID && HAPTIC_PIDS.contains(&dev.product_id()))
}

// Where fwupd's `pixart-tp` plugin reads the firmware version.
const REGISTER_READ: u8 = 0x10;
const VERSION_LOW: u8 = 0xb2;
const VERSION_HIGH: u8 = 0xb3;

/// The pad's firmware version in hex, as fwupd spells it without its `0x`;
/// None where a register would not answer.
pub(crate) fn firmware(hid: &hidapi::HidApi, pad: &hidapi::DeviceInfo) -> Option<String> {
    let device = pad.open_device(hid).ok()?;
    let low = read_register(&device, VERSION_LOW)?;
    let high = read_register(&device, VERSION_HIGH)?;
    Some(version(low, high))
}

fn read_register(device: &hidapi::HidDevice, address: u8) -> Option<u8> {
    device
        .send_feature_report(&[touchpad::P274_REPORT_ID, address, REGISTER_READ, 0])
        .ok()?;
    let mut reply = [touchpad::P274_REPORT_ID, 0, 0, 0];
    let read = device.get_feature_report(&mut reply).ok()?;
    (read == reply.len()).then_some(reply[3])
}

fn version(low: u8, high: u8) -> String {
    format!("{:04X}", u16::from_le_bytes([low, high]))
}

pub(crate) fn click_force(force: contract::ClickForce) -> ClickForce {
    match force {
        contract::ClickForce::Low => ClickForce::Low,
        contract::ClickForce::Medium => ClickForce::Medium,
        contract::ClickForce::High => ClickForce::High,
    }
}

/// The device code the state file carries, back to the contract's name; None for
/// a code no force maps to.
pub(crate) fn contract_click_force(code: u8) -> Option<contract::ClickForce> {
    contract::ClickForce::ALL
        .into_iter()
        .find(|force| click_force(*force) as u8 == code)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_version_is_the_two_registers_low_byte_first() {
        assert_eq!(super::version(0x10, 0x13), "1310");
    }

    /// The app offers these steps but cannot link `framework_lib` to learn
    /// them, so `contract` carries the list and this is what keeps the copy
    /// honest. A firmware generation that changes the steps should fail here
    /// rather than in a combo that silently offers the wrong ones.
    #[test]
    fn the_wire_haptic_steps_are_the_ones_the_touchpad_implements() {
        assert_eq!(
            frameguin_contract::HAPTIC_INTENSITY_LEVELS,
            framework_lib::touchpad::HAPTIC_INTENSITY_LEVELS
        );
    }

    /// The store carries the device's own code, so the default has to map
    /// to one and back.
    #[test]
    fn the_stored_default_is_the_force_the_getter_names() {
        assert_eq!(
            super::contract_click_force(super::click_force(super::DEFAULT_CLICK_FORCE) as u8),
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
