//! Which way into the touch panel this machine has, and the role a device
//! holds it through.
//!
//! One control, reached two unrelated ways: a pad on the processor
//! ([`crate::gpio`]) where the panel takes no command, the panel's own
//! transport ([`crate::panel`]) where it does. A machine has one or the
//! other and never both — which pad carries the enable is a fact about the
//! mainboard, whether a command is implemented is a fact about the panel, and
//! the pairings that exist put exactly one of the two within reach. So this
//! is a precedence rather than a handover.
//!
//! What the routes do not share is readback: the pad holds the level it is
//! driving and the panel holds nothing, so only one of them can be asked
//! what it is. That difference is stated once, in [`TouchSwitch::reading`],
//! and everything that follows from it — which account answers a getter,
//! whether a write can be skipped as already in place — is read off that one
//! answer rather than decided again per call site.

use frameguin_wire::DeviceResult;
use framework_lib::touchscreen::{HX_PID, HX_VID};

use crate::{gpio, panel};

/// What a device needs of the way in: the reading the route affords, and
/// the switch itself.
pub trait TouchSwitch: Send + Sync {
    /// What the hardware itself holds, and None on a route that holds
    /// nothing.
    fn reading(&self) -> DeviceResult<Option<bool>>;
    fn set_enabled(&self, enabled: bool) -> DeviceResult<()>;
}

/// The way in, as [`find`] settled it.
pub enum Route {
    Pad(gpio::Pad),
    Panel,
}

/// Touch controllers a board's enable pad is what gates, by the identity they
/// announce on the bus.
///
/// Keyed on the controller rather than on which panel it shipped in, as the
/// haptic touchpad is: the enable is a board signal reaching the display
/// connector, so it gates whichever touch panel is plugged into it.
///
/// A controller belongs here when the pad is how this daemon switches it, and
/// so not the Ilitek, which answers a command of its own — [`crate::panel`]
/// curates that route's controller by the same rule and for the same reason,
/// beside the code that sends it the command.
const GATED_CONTROLLERS: [(u16, u16); 1] = [(HX_VID, HX_PID)];

/// The route this machine has, if it has one worth offering, and the
/// controller behind it — the part a person bought.
///
/// The pad is asked first because it is the precedence: a board that has
/// the pad has no panel command behind it, so nothing about the panel can
/// change the answer once the pad is found.
///
/// A pad found is the route whether or not it qualifies, since the pairings
/// that exist put no panel command behind a board that has the pad. What
/// qualifies it is more than finding it: the board naming the pad is only
/// half, and the controller on the bus is what says anything is behind the
/// line — panels and mainboards are sold apart and the chassis takes any
/// pairing, so a board of the right generation behind a panel with no touch
/// would otherwise be offered a switch with nothing on the end of it. The
/// reading is the setter's own line request, side-effect-free and failing on
/// everything the write would fail on: a chip that will not open, a locked
/// pad, a line another driver holds.
///
/// The panel needs nothing added: the command is the controller's own, so
/// finding the controller is the whole question.
pub fn find(hid: &hidapi::HidApi) -> Option<(Route, &hidapi::DeviceInfo)> {
    if let Some(pad) = gpio::touchscreen() {
        let controller = gated_controller(hid)?;
        return pad.level().is_ok().then_some((Route::Pad(pad), controller));
    }
    panel::controller(hid).map(|controller| (Route::Panel, controller))
}

fn gated_controller(hid: &hidapi::HidApi) -> Option<&hidapi::DeviceInfo> {
    hid.device_list()
        .find(|dev| GATED_CONTROLLERS.contains(&(dev.vendor_id(), dev.product_id())))
}

impl Route {
    /// The controller's firmware version, read the way its vendor answers
    /// it — which the route already settles, the pad gating a Himax and
    /// the command being the Ilitek's. None where it would not answer.
    pub fn firmware(
        &self,
        hid: &hidapi::HidApi,
        controller: &hidapi::DeviceInfo,
    ) -> Option<String> {
        let device = controller.open_device(hid).ok()?;
        match self {
            Self::Pad(_) => panel::himax_firmware(&device),
            Self::Panel => panel::ilitek_firmware(&device),
        }
    }
}

impl TouchSwitch for Route {
    fn reading(&self) -> DeviceResult<Option<bool>> {
        match self {
            Self::Pad(pad) => Ok(Some(pad.level()?)),
            Self::Panel => Ok(None),
        }
    }

    fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        match self {
            Self::Pad(pad) => Ok(pad.drive(enabled)?),
            Self::Panel => Ok(panel::set_enabled(enabled)?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GATED_CONTROLLERS, Route, TouchSwitch};
    use crate::gpio::Pad;
    use crate::panel::COMMANDED_CONTROLLER;

    #[test]
    fn a_panel_route_holds_nothing_to_read() {
        assert_eq!(Route::Panel.reading(), Ok(None));
    }

    /// A pad that cannot be read must not answer as a route holding nothing:
    /// the device reads its own mirror on that answer, where the pad is the
    /// account it should be failing on.
    #[test]
    fn a_pad_that_cannot_be_read_fails_rather_than_holding_nothing() {
        assert!(Route::Pad(Pad::unopenable()).reading().is_err());
    }

    /// A controller in both tables would be switched by the pad on a board
    /// that has one, though its own command works.
    #[test]
    fn the_two_routes_never_claim_the_same_controller() {
        assert!(!GATED_CONTROLLERS.contains(&COMMANDED_CONTROLLER));
    }
}
