//! The touch panel's D-Bus half: the route as the bus surface needs it, and
//! the write that records itself on the route with no readback.
//!
//! Which route a machine has, and what it can read, is
//! [`frameguin_hardware::touchscreen`]'s; what is here is what makes a
//! module about hardware reach into the daemon's own state — the panel
//! holds nothing, so the mirror is the only account of it.

use frameguin_hardware::lifetime::HostStamp;
use frameguin_hardware::panel;
use frameguin_hardware::touchscreen::{self, Route};
use zbus::fdo;

use crate::{Daemon, internal_err};

/// The route, as the D-Bus surface needs it: the permanent refusal hardware
/// with neither route gets is `NotSupported` rather than `Failed`, so a
/// caller can tell wrong hardware from an attempt that went wrong.
pub(crate) fn route() -> fdo::Result<Route> {
    touchscreen::find(None).ok_or_else(|| {
        fdo::Error::NotSupported("no way to switch the touch panel on this hardware".into())
    })
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
                // Dated before the write, so that a restart between the two
                // reads as having dropped it.
                let stamp = if enabled { None } else { HostStamp::now() };
                panel::set_enabled(enabled).map_err(internal_err)?;
                *self.touchscreen_off.lock().unwrap() = stamp;
                self.save_state();
                Ok(())
            }
        }
    }
}
