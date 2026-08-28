//! The touch panel's own HID transport, which addresses the controller
//! rather than the board.
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
//! mirror in [`crate::state`].

use std::io;
use std::sync::OnceLock;

use framework_lib::touchscreen;

/// What an enumeration said, once one has run. Which panel is fitted cannot
/// change while this daemon lives, and finding out costs a walk of every HID
/// device on the machine — the most expensive question it asks.
///
/// Only an enumeration that actually ran is remembered. Failing to enumerate
/// at all leaves the question open, so a transient failure cannot deny the
/// control for a whole daemon lifetime, which is what the probe rule reserves.
static FITTED: OnceLock<bool> = OnceLock::new();

/// The usage page `framework_lib` picks the controller's vendor collection
/// out by. Restated rather than imported because its own constant is
/// private, and a probe has to look for the device the setter will open
/// rather than for one like it.
const VENDOR_USAGE_PAGE: u16 = 0xFF00;

/// Whether a controller that takes the enable command is on the bus.
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
/// recognize. That is the setter's business and this is the offer's, and the
/// two are not the same question — a controller missing from what is named
/// here is a control not shown, never a write refused, exactly as
/// [`crate::probe`]'s own list of them says. Left off, the offer would rest
/// on a vendor id this vendor sells to the whole industry and a usage page
/// every vendor collection uses, so a laptop of no relation carrying one of
/// their panels would be shown the control — and taking it would send the
/// command to a controller this daemon cannot identify.
///
/// The usage page stays in the question because it is how the setter picks
/// the collection to open, so this looks for the device that write will land
/// on rather than for one like it.
///
/// Takes an enumeration already in hand where there is one, as
/// [`crate::touchpad`] does, and makes one only where there is not.
pub fn present(hid: Option<&hidapi::HidApi>) -> bool {
    if let Some(&fitted) = FITTED.get() {
        return fitted;
    }
    let ran = match hid {
        Some(hid) => Some(scan(hid)),
        None => hidapi::HidApi::new().ok().map(|hid| scan(&hid)),
    };
    let Some(fitted) = ran else {
        return false;
    };
    *FITTED.get_or_init(|| fitted)
}

fn scan(hid: &hidapi::HidApi) -> bool {
    hid.device_list().any(|dev| {
        dev.vendor_id() == touchscreen::ILI_VID
            && dev.product_id() == touchscreen::ILI_PID
            && dev.usage_page() == VENDOR_USAGE_PAGE
    })
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
