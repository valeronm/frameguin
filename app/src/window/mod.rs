//! The preferences window: the `Ui` its groups share, the `Sink` a write
//! reports to, and the window built around them.
//!
//! [`Sink`] lives here rather than beside the tray because the window is the
//! end with somewhere to report; a group's `apply` takes one, so a tray
//! preset borrows the window when one has been built and answers for itself
//! when one has not.
//!
//! A control with a module of its own in `frameguin_model` has a group
//! module of its own here — its widgets, how a snapshot moves them, and what
//! its handlers dispatch.

pub(crate) mod battery;
mod fill;
pub(crate) mod power_led;
mod touchpad;
pub(crate) mod touchscreen;
mod widgets;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use frameguin_model::control::Controls;
use frameguin_wire::DeviceError;
use gtk4 as gtk;
use gtk4::gio;

use crate::bus::Bus;
use crate::daemon::Daemon;
use crate::mapped::Once;
use crate::reading::Feed;
use crate::report::parts;
use crate::tray::{TrayIcon, TrayValues, tray_push};
use crate::window::widgets::debounce;
use crate::{APP_ID, autostart, board};

/// The one sentence the header and the empty page both say, so a reword
/// cannot leave the window carrying two versions of it.
const NO_HARDWARE: &str = "No Framework hardware detected";

pub(crate) struct Ui {
    toasts: adw::ToastOverlay,
    /// Set while widgets are being moved to mirror the hardware, so their
    /// change handlers don't echo the reading back as a write.
    syncing: Cell<bool>,
    battery: battery::Group,
    power_led: power_led::Group,
    touchpad: touchpad::Group,
    touchscreen: touchscreen::Group,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
    daemon: Rc<Daemon>,
    /// Where the status row's reading comes from, shared with the battery
    /// report so the two windows cost the EC one walk between them rather than
    /// one apiece.
    feed: Rc<Feed>,
}

impl Ui {
    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    /// A call that failed, named by what was being attempted — "Setting the
    /// charge limit", "Reading the battery". The sentence is built here rather
    /// than at each site so that every failure loses the D-Bus error name and
    /// none has to remember to; what each site still spells is its own half,
    /// which is the half no other site could supply. Takes a bus error or a
    /// device's `DeviceError` alike, the conversion being what drops the name.
    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        self.toast(&format!("{attempt} failed: {}", error.into()));
    }

    /// Moves widgets to match the hardware without their handlers writing the
    /// value straight back. Every setter returns early while this is set.
    fn sync(&self, update: impl FnOnce()) {
        self.syncing.set(true);
        update();
        self.syncing.set(false);
    }

    fn sync_tray(&self, values: TrayValues) {
        if let Some(handle) = &self.tray {
            tray_push(handle, values);
        }
    }

    /// Shows each group where its control is.
    fn gate(&self, controls: &Controls<Bus>) {
        self.battery.gate(controls.battery.as_ref());
        self.power_led.gate(controls.power_led.as_ref());
        self.touchpad.gate(controls.touchpad.as_ref());
        self.touchscreen.gate(controls.touchscreen.as_ref());
    }

    /// Re-reads every detected control and moves the widgets to match,
    /// pushing the same values to the tray along with what each control
    /// offers. Each write goes through [`Ui::sync`], so a reload can't echo
    /// back as a setter call. The tray's copies are collected and handed over
    /// in one go at the end: each push blocks on the tray's thread and
    /// rebuilds its whole menu, which would be wasted once per control on a
    /// menu nobody has opened.
    async fn load_values(&self, controls: &Controls<Bus>) {
        let mut values = TrayValues::offered(controls);
        if let Some(battery) = &controls.battery {
            self.battery.load(self, battery, &mut values).await;
        }
        if let Some(power_led) = &controls.power_led {
            self.power_led.load(self, power_led, &mut values).await;
        }
        if let Some(touchpad) = &controls.touchpad {
            self.touchpad.load(self, touchpad).await;
        }
        if let Some(touchscreen) = &controls.touchscreen {
            self.touchscreen.load(self, touchscreen, &mut values).await;
        }
        self.sync_tray(values);
    }

    /// A control's group connects only where the control is, its handlers
    /// needing one to dispatch to.
    fn connect_handlers(self: &Rc<Self>, controls: &Controls<Bus>) {
        if let Some(battery) = &controls.battery {
            self.battery.connect(self, battery);
        }
        if let Some(power_led) = &controls.power_led {
            self.power_led.connect(self, power_led);
        }
        if let Some(touchpad) = &controls.touchpad {
            self.touchpad.connect(self, touchpad);
        }
        if let Some(touchscreen) = &controls.touchscreen {
            self.touchscreen.connect(self, touchscreen);
        }
    }
}

/// Where a write reports back to. A tray preset can arrive in a session whose
/// window has never been built, and building a widget tree to hold a toast
/// nobody will see is not worth it — so the tray answers for itself, and only
/// the window carries the parts a window has. What the tray has is the
/// desktop's notifications, and only a refusal earns one — the menu
/// retitling itself is the success.
#[derive(Clone, Copy)]
pub(crate) enum Sink<'a> {
    Window(&'a Ui),
    Tray {
        handle: &'a ksni::blocking::Handle<TrayIcon>,
        app: &'a adw::Application,
        /// The pending withdrawal of the last refusal, which the next one
        /// re-arms.
        withdraw: &'a Cell<Option<Once>>,
    },
}

/// One id for every refusal, so a second replaces the first on the shell
/// rather than stacking beside it.
const REFUSAL_NOTIFICATION: &str = "write-refused";

/// The shell keeps a notification until it is dismissed, and a refusal is
/// not worth dismissing by hand.
const WITHDRAW_REFUSAL_AFTER: Duration = Duration::from_secs(5);

/// A refused write, told from the tray: the desktop's notification, the one
/// channel a session with no window has.
fn notify_refusal(
    app: &adw::Application,
    withdraw: &Cell<Option<Once>>,
    attempt: &str,
    error: &DeviceError,
) {
    // An uninstalled build sends this into nothing: the shell delivers a
    // notification only for an app whose desktop file it can find.
    let notification = gio::Notification::new(&format!("{attempt} failed"));
    notification.set_body(Some(&error.to_string()));
    app.send_notification(Some(REFUSAL_NOTIFICATION), &notification);
    let app = app.clone();
    debounce(withdraw, WITHDRAW_REFUSAL_AFTER, move || {
        app.withdraw_notification(REFUSAL_NOTIFICATION);
    });
}

impl Sink<'_> {
    fn toast(&self, message: &str) {
        if let Sink::Window(ui) = self {
            ui.toast(message);
        }
    }

    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        match self {
            Sink::Window(ui) => ui.toast_error(attempt, error),
            Sink::Tray { app, withdraw, .. } => {
                notify_refusal(app, withdraw, attempt, &error.into());
            }
        }
    }

    /// Sends what this sink can vouch for to the tray, wherever it lives.
    fn push_tray(&self, values: TrayValues) {
        match self {
            Sink::Window(ui) => ui.sync_tray(values),
            Sink::Tray { handle, .. } => tray_push(handle, values),
        }
    }
}

pub(crate) fn build_window(
    app: &adw::Application,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
    daemon: Rc<Daemon>,
    feed: Rc<Feed>,
) -> (adw::ApplicationWindow, Rc<Ui>) {
    let page = adw::PreferencesPage::new();

    let battery = battery::Group::build();
    page.add(&battery.widget);

    let power_led = power_led::Group::build();
    page.add(&power_led.widget);

    let touchpad = touchpad::Group::build();
    page.add(&touchpad.widget);

    let touchscreen = touchscreen::Group::build();
    page.add(&touchscreen.widget);

    let application = adw::PreferencesGroup::builder()
        .title("Application")
        .build();
    let autostart_row = adw::SwitchRow::builder()
        .title("Start at login")
        .subtitle("Show only the tray icon until opened")
        .build();
    autostart_row.set_active(autostart::entry_path().exists());
    application.add(&autostart_row);
    page.add(&application);

    // Detected hardware as the header subtitle: one line, no key/value rows.
    let detected = board::detected().unwrap_or_else(|| NO_HARDWARE.to_string());

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Frameguin", &detected)));

    let menu = gio::Menu::new();
    menu.append(Some("_Parts"), Some(&format!("app.{}", parts::ACTION)));
    menu.append(Some("_About Frameguin"), Some("app.about"));
    menu.append(Some("_Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build();
    header.pack_end(&menu_button);
    view.add_top_bar(&header);
    let empty = fill::build_empty_page(&view, &page);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Frameguin")
        .default_width(420)
        // Tall enough for every control group at the default font scale;
        // re-measure when the rows change.
        .default_height(710)
        .content(&toasts)
        .icon_name(APP_ID)
        .build();

    // Hiding instead of closing only makes sense while a tray icon exists to
    // bring the window back.
    window.set_hide_on_close(tray.is_some());

    let ui = Rc::new(Ui {
        toasts,
        syncing: Cell::new(false),
        battery,
        power_led,
        touchpad,
        touchscreen,
        tray,
        daemon,
        feed,
    });

    let autostart_ui = ui.clone();
    autostart_row.connect_active_notify(move |row| {
        if let Err(e) = autostart::set(row.is_active()) {
            // Quoted off the row rather than spelled again: a message naming
            // a row by a title the row no longer has is worse than a vaguer
            // one.
            autostart_ui.toast(&format!("Setting “{}” failed: {e}", row.title()));
        }
    });

    fill::attach(&window, &ui, empty);

    (window, ui)
}
