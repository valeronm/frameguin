//! Which way into the touch panel this machine has, and the reading that way
//! affords.
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
//! what it is. That difference is stated once, in [`Route::reading`], and
//! everything that follows from it — which account answers a getter, whether
//! a write can be skipped as already in place — is read off that one answer
//! rather than decided again per call site.

use frameguin_wire::DeviceResult;

use crate::{gpio, panel};

/// The way in, held for the length of one operation.
pub enum Route {
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
/// one is fitted cannot change while this process lives — [`panel::present`]
/// answers from the first enumeration that ran.
///
/// `hid` is an enumeration already in hand, for a caller that has one.
pub fn find(hid: Option<&hidapi::HidApi>) -> Option<Route> {
    if let Some(pad) = gpio::touchscreen() {
        return Some(Route::Pad(pad));
    }
    panel::present(hid).then_some(Route::Panel)
}

impl Route {
    /// What the hardware itself says, and None on the panel route, which
    /// answers nothing. A getter falls back to the mirror where there is no
    /// reading, and a setter skips a write only where a reading equals what
    /// was asked, so the panel skips none.
    pub fn reading(&self) -> DeviceResult<Option<bool>> {
        match self {
            Self::Pad(pad) => Ok(Some(pad.level()?)),
            Self::Panel => Ok(None),
        }
    }
}
