//! How a failed daemon call is told to whoever asked for it: the sentence
//! both tellings open with, the toast a window shows, and the desktop
//! notification a session with no window has instead.
//!
//! A failure with no device behind it — a browser that would not open, a
//! desktop entry that would not write — words itself where it happens: the
//! `DeviceError` these take is what drops the D-Bus error name, and there is
//! none to drop.

use std::cell::Cell;
use std::time::Duration;

use adw::prelude::*;
use frameguin_wire::DeviceError;
use gtk4::gio;

use crate::mapped::Once;

/// What each site spells is its own half — "Setting the charge limit",
/// "Reading the battery" — which is the half no other site could supply.
fn headline(attempt: &str) -> String {
    format!("{attempt} failed")
}

/// A failed call, told in the window that asked for it. Takes a bus error or
/// a device's `DeviceError` alike, the conversion being what drops the D-Bus
/// error name in front of the sentence.
pub(crate) fn toast(toasts: &adw::ToastOverlay, attempt: &str, error: impl Into<DeviceError>) {
    let message = format!("{}: {}", headline(attempt), error.into());
    toasts.add_toast(adw::Toast::new(&message));
}

/// One id for every refusal, so a second replaces the first on the shell
/// rather than stacking beside it.
const REFUSAL: &str = "write-refused";

/// The shell keeps a notification until it is dismissed, and a refusal is
/// not worth dismissing by hand.
const WITHDRAW_REFUSAL_AFTER: Duration = Duration::from_secs(5);

/// The desktop's notifications, and the withdrawal pending for the last
/// refusal sent to them — one holder, so the id, the delay and what is armed
/// against them cannot come apart.
pub(crate) struct Notifier {
    app: adw::Application,
    withdraw: Cell<Option<Once>>,
}

impl Notifier {
    pub(crate) fn new(app: adw::Application) -> Self {
        Self {
            app,
            withdraw: Cell::default(),
        }
    }

    /// A refused write, told from the tray: the desktop's notification, the
    /// one channel a session with no window has.
    pub(crate) fn refused(&self, attempt: &str, error: &DeviceError) {
        // An uninstalled build sends this into nothing: the shell delivers a
        // notification only for an app whose desktop file it can find.
        let notification = gio::Notification::new(&headline(attempt));
        notification.set_body(Some(&error.to_string()));
        self.app.send_notification(Some(REFUSAL), &notification);
        let app = self.app.clone();
        self.withdraw
            .set(Some(Once::after(WITHDRAW_REFUSAL_AFTER, move || {
                app.withdraw_notification(REFUSAL);
            })));
    }
}
