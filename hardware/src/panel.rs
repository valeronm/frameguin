//! The touch controllers' HID transport, which addresses the controller
//! rather than the board: the Ilitek's enable command, and the version
//! reads of both controllers.
//!
//! Only one of the controllers these machines ship with takes a command at
//! all. The Ilitek implements a vendor report that stops it reporting; the
//! Himax implements nothing of the kind, which is why [`crate::gpio`]'s route
//! exists. So this is not the general way to switch a panel — it is the way
//! to switch this controller, and the identity below is what holds those
//! apart. `docs/hardware.md` carries the evidence.
//!
//! Write-only, unlike the pad: the command asks for no reply and the
//! controller volunteers none, so what was set is knowable only from the
//! mirror [`crate::device::touchscreen`] keeps.

use std::io;

use framework_lib::touchscreen;

/// The usage page `framework_lib` picks the controller's vendor collection
/// out by. Restated rather than imported because its own constant is
/// private, and detection has to look for the device the setter will open
/// rather than for one like it.
const VENDOR_USAGE_PAGE: u16 = 0xFF00;

/// The controller that takes the enable command, if one is on the bus.
///
/// Curated knowledge, per the probe rule: nothing side-effect-free can ask a
/// panel whether it implements the command, and the read that looks as
/// though it could — the version report — is answered by controllers that do
/// not. Keyed on the controller rather than on the board for the reason the
/// pad is keyed the other way round: the command belongs to the panel, and
/// panels and mainboards are sold apart.
///
/// The product id is curated rather than dropped, though the setter would
/// open a device without it: `framework_lib` only warns about one it does not
/// recognize. Left off, this would rest on a vendor id this vendor sells to
/// the whole industry and a usage page every vendor collection uses, so a
/// laptop of no relation carrying one of their panels would be offered the
/// control — and taking it would send the command to a controller this
/// daemon cannot identify.
pub fn controller(hid: &hidapi::HidApi) -> Option<&hidapi::DeviceInfo> {
    hid.device_list().find(|dev| {
        (dev.vendor_id(), dev.product_id()) == COMMANDED_CONTROLLER
            && dev.usage_page() == VENDOR_USAGE_PAGE
    })
}

pub(crate) const COMMANDED_CONTROLLER: (u16, u16) = (touchscreen::ILI_VID, touchscreen::ILI_PID);

/// The Ilitek's vendor protocol: a feature report carrying a message id,
/// its argument and how many bytes to read back, answered on the input
/// endpoint behind an echo of the header and argument.
const ILITEK_REPORT_ID: u8 = 0x03;
const ILITEK_MESSAGE: u8 = 0xA3;
const ILITEK_ARGUMENT_LEN: u8 = 1;
const ILITEK_FIRMWARE_VERSION: u8 = 0x40;
const ILITEK_VERSION_LEN: u8 = 8;
const ILITEK_BUF_LEN: usize = 0x40;
const ILITEK_REPLY_HEADER: usize = 4;
/// Long enough for the controller to answer, short enough that one asleep
/// does not hold detection.
const ILITEK_READ_TIMEOUT_MS: i32 = 1000;

/// The Ilitek's firmware version, as eight decimal fields.
pub fn ilitek_firmware(device: &hidapi::HidDevice) -> Option<String> {
    let mut message = [0u8; ILITEK_BUF_LEN];
    message[0] = ILITEK_REPORT_ID;
    message[1] = ILITEK_MESSAGE;
    message[2] = ILITEK_ARGUMENT_LEN;
    message[3] = ILITEK_VERSION_LEN;
    message[4] = ILITEK_FIRMWARE_VERSION;
    device.send_feature_report(&message).ok()?;
    let mut reply = [0u8; ILITEK_BUF_LEN];
    device
        .read_timeout(&mut reply, ILITEK_READ_TIMEOUT_MS)
        .ok()?;
    let fields =
        reply.get(ILITEK_REPLY_HEADER..ILITEK_REPLY_HEADER + usize::from(ILITEK_VERSION_LEN))?;
    Some(ilitek_version(fields))
}

fn ilitek_version(fields: &[u8]) -> String {
    fields
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

/// The Himax's configuration report, and where in it the chip id — what
/// its vendor calls the firmware version — sits.
const HIMAX_REPORT_ID_CFG: u8 = 0x05;
const HIMAX_CFG_LEN: usize = 256;
const HIMAX_CID_OFFSET: usize = 52;

/// The Himax's firmware version, as its chip id in hex.
pub fn himax_firmware(device: &hidapi::HidDevice) -> Option<String> {
    let mut buf = [0u8; HIMAX_CFG_LEN];
    buf[0] = HIMAX_REPORT_ID_CFG;
    let read = device.get_feature_report(&mut buf).ok()?;
    himax_version(buf.get(1..read)?)
}

fn himax_version(config: &[u8]) -> Option<String> {
    let cid = config
        .get(HIMAX_CID_OFFSET..HIMAX_CID_OFFSET + 2)?
        .try_into()
        .ok()?;
    Some(format!("{:04X}", u16::from_be_bytes(cid)))
}

/// Tells the controller to stop or resume reporting.
///
/// `framework_lib` finds and opens the device itself, and unwraps that open:
/// a controller that enumerates but will not open would take this daemon
/// down rather than return an error. Running as root against hidraw is what
/// keeps that theoretical.
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    touchscreen::enable_touch(enabled)
        .ok_or_else(|| io::Error::other("the touch panel refused the enable command"))
}

#[cfg(test)]
mod tests {
    use super::{HIMAX_CID_OFFSET, himax_version, ilitek_version};

    #[test]
    fn an_ilitek_version_is_its_fields_in_decimal() {
        assert_eq!(
            ilitek_version(&[6, 0, 10, 0, 0, 1, 0, 0]),
            "6.0.10.0.0.1.0.0"
        );
    }

    #[test]
    fn a_himax_version_is_the_chip_id_in_hex() {
        let mut config = vec![0u8; HIMAX_CID_OFFSET + 2];
        config[HIMAX_CID_OFFSET] = 0x0A;
        config[HIMAX_CID_OFFSET + 1] = 0x1B;
        assert_eq!(himax_version(&config).as_deref(), Some("0A1B"));
    }

    #[test]
    fn a_short_himax_report_has_no_version() {
        assert_eq!(himax_version(&[0; HIMAX_CID_OFFSET + 1]), None);
    }
}
