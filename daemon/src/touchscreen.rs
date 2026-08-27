//! Which way into the touch panel this machine has, and the reading that way
//! affords.
//!
//! One control and one capability, reached two unrelated ways: a pad on the
//! processor ([`crate::gpio`]) where the panel takes no command, the panel's
//! own transport ([`crate::panel`]) where it does. A machine has one or the
//! other and never both — which pad carries the enable is a fact about the
//! mainboard, whether a command is implemented is a fact about the panel, and
//! the pairings that exist put exactly one of the two within reach. So this
//! is a precedence rather than the handover [`crate::power_led`] arbitrates,
//! where two drivers hold one LED in turn and the order they change hands in
//! is the whole of it.
//!
//! What the routes do not share is readback, which is why a module about
//! hardware reaches into the daemon's own state: the pad holds the level it
//! is driving and the panel holds nothing, so only one of them can be asked
//! what it is. That difference is stated once, in [`Route::reading`], and
//! everything that follows from it — which account answers a getter, whether
//! a write can be skipped as already in place — is read off that one answer
//! rather than decided again per call site.

use zbus::fdo;

use crate::host::BootStamp;
use crate::{Daemon, gpio, internal_err, panel};

/// The way in, held for the length of one operation.
pub(crate) enum Route {
    Pad(gpio::Pad),
    Panel,
}

/// The route this machine has, if it has one.
///
/// Precedence and nothing else. What makes a route worth *offering* is more
/// than what makes it usable, and that surplus is [`crate::probe`]'s — which
/// asks this first, so the route it qualifies is the route an operation will
/// then take. Written out separately at both ends, the two could qualify one
/// route and use the other on a pairing neither of them named.
///
/// The pad is asked first because asking is nearly free: a single DMI read
/// that every board but one fails, which spares the rest the panel's
/// question entirely.
///
/// The pad is looked up afresh every call, because what it answers can change
/// between two: one some driver has claimed since is meant to fail at its own
/// line request rather than be written on the strength of what was true at
/// startup, per [`crate::gpio::touchscreen`]. The panel is not, because which
/// one is fitted cannot change while this daemon lives — [`panel::present`]
/// answers from the first enumeration that ran.
///
/// `hid` is an enumeration already in hand, for a caller that has one.
pub(crate) fn find(hid: Option<&hidapi::HidApi>) -> Option<Route> {
    if let Some(pad) = gpio::touchscreen() {
        return Some(Route::Pad(pad));
    }
    panel::present(hid).then_some(Route::Panel)
}

/// The same, as the D-Bus surface needs it: the permanent refusal hardware
/// with neither route gets is `NotSupported` rather than `Failed`, so a
/// caller can tell wrong hardware from an attempt that went wrong.
pub(crate) fn route() -> fdo::Result<Route> {
    find(None).ok_or_else(|| {
        fdo::Error::NotSupported("no way to switch the touch panel on this hardware".into())
    })
}

impl Route {
    /// What the hardware itself says, and None where it says nothing.
    ///
    /// The whole of the asymmetry, in the shape it really has: three answers
    /// rather than two. The pad reports the level it is driving, so a value
    /// set before this daemon started — or by the firmware at power-on —
    /// reads the same as one written here. The panel is asked nothing,
    /// because it answers nothing.
    ///
    /// Both things the daemon does with the state are read off this. A getter
    /// falls back to the mirror where there is no reading; a setter skips a
    /// write only where a reading equals what was asked, so the panel never
    /// skips one. Its mirror is dated against the boot, which is what stops
    /// it outliving a reboot, but nothing dates it against the suspend or the
    /// firmware that can move the panel inside one — so within a boot it is
    /// no fresher than the caller's own idea of the value, and skipping on it
    /// would drop exactly the write that would have corrected it, reporting
    /// success for doing so. The charge current limit may be skipped on
    /// because the EC restart that invalidates it is the same event its stamp
    /// catches; here the two come apart.
    pub(crate) fn reading(&self) -> fdo::Result<Option<bool>> {
        match self {
            Self::Pad(pad) => pad.level().map(Some).map_err(internal_err),
            Self::Panel => Ok(None),
        }
    }
}

impl Daemon {
    /// Switches touch, and on the route that keeps no record of it, records
    /// the write. Saved rather than only held, because this daemon exits
    /// after five idle minutes: a mirror that lived in the process would
    /// answer "on" every time one of those had gone by.
    pub(crate) fn write_touchscreen(&self, route: &Route, enabled: bool) -> fdo::Result<()> {
        match route {
            Route::Pad(pad) => pad.drive(enabled).map_err(internal_err),
            Route::Panel => {
                // Dated before the write rather than after it, as the power
                // LED's darkness is. A date that cannot be taken costs the
                // record and never the write: what the panel is sent is a
                // state rather than a gesture, so asserting off on a panel
                // already off costs nothing, and turning it back on needs no
                // date at all — where refusing would deny a write the
                // hardware would have taken, to protect a label that is
                // approximate anyway, a suspend being able to move the panel
                // inside a boot with this none the wiser.
                let stamp = if enabled { None } else { BootStamp::now() };
                panel::set_enabled(enabled).map_err(internal_err)?;
                *self.touchscreen_off.lock().unwrap() = stamp;
                self.save_state();
                Ok(())
            }
        }
    }
}
