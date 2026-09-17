//! The main window: the `Ui` its groups share, the `Sink` a write reports
//! to, and the window built around them.
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
mod preferences;
mod touchpad;
pub(crate) mod touchscreen;
mod widgets;

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::Controls;
use frameguin_wire::DeviceError;
use gtk4 as gtk;
use gtk4::gio;

use crate::APP_ID;
use crate::bus::Bus;
use crate::daemon::Daemon;
use crate::failure::{self, Notifier};
use crate::reading::Feed;
use crate::report::{parts, status};
use crate::tray::{TrayIcon, TrayValues, tray_push};

pub(crate) struct Ui {
    toasts: adw::ToastOverlay,
    /// Set while widgets are being moved to mirror the hardware, so their
    /// change handlers don't echo the reading back as a write.
    syncing: Cell<bool>,
    header: adw::HeaderBar,
    switcher: adw::ViewSwitcher,
    stack: adw::ViewStack,
    tabs: Vec<Tab>,
    battery: battery::Group,
    power_led: power_led::Group,
    touchpad: touchpad::Group,
    touchscreen: touchscreen::Group,
    preferences: preferences::Group,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
    daemon: Rc<Daemon>,
    feed: Rc<Feed>,
}

impl Ui {
    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        self.toasts.add_toast(failure::toast(attempt, error));
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

    /// Shows each group where its control is, each tab where one of its
    /// groups is, and the switcher where more than one tab is left to switch
    /// between.
    fn gate(&self, controls: &Controls<Bus>) {
        self.battery
            .gate(controls.battery.as_ref(), controls.ports.as_ref());
        self.power_led.gate(controls.power_led.as_ref());
        self.touchpad.gate(controls.touchpad.as_ref());
        self.touchscreen.gate(controls.touchscreen.as_ref());
        let mut shown = 0;
        for tab in &self.tabs {
            let visible = tab.groups.iter().any(WidgetExt::is_visible);
            tab.page.set_visible(visible);
            shown += usize::from(visible);
        }
        // Swapped out rather than hidden, so the header falls back to the
        // window's title instead of showing none.
        self.header
            .set_title_widget((shown > 1).then_some(&self.switcher));
    }

    fn show_first_tab(&self) {
        if let Some(tab) = self.tabs.iter().find(|tab| tab.page.is_visible()) {
            self.stack.set_visible_child(&tab.page.child());
        }
    }

    /// Subscribes every group's fed rows to the reading. Its own fan-out
    /// rather than part of `connect_handlers`, which runs last so a loaded
    /// value cannot echo back as a write — nothing here writes.
    ///
    /// Takes no controls: a subscription is held only while its widget is
    /// mapped, so `gate` having left an unsupported row off the screen is
    /// already the gate.
    fn watch(self: &Rc<Self>) {
        self.battery.watch(self);
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
        // Read through the feed rather than from a control, so it fills on a
        // board that has either of the two devices its rows show.
        if battery::shown(controls.battery.as_ref(), controls.ports.as_ref()) {
            self.battery.load_fed(self, &mut values).await;
        }
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

    async fn load_preferences(&self) {
        self.preferences.load(self).await;
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
        self.preferences.connect(self);
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
        notifier: &'a Notifier,
    },
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
            Sink::Tray { notifier, .. } => notifier.refused(attempt, &error.into()),
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

/// Recorded where its groups are added, so whether the tab shows cannot
/// disagree with what it holds.
struct Tab {
    page: adw::ViewStackPage,
    groups: Vec<adw::PreferencesGroup>,
}

fn add_tab(
    stack: &adw::ViewStack,
    title: &str,
    icon: &str,
    groups: &[&adw::PreferencesGroup],
) -> Tab {
    let page = adw::PreferencesPage::new();
    for group in groups {
        page.add(*group);
    }
    Tab {
        page: stack.add_titled_with_icon(&page, None, title, icon),
        groups: groups.iter().map(|&group| group.clone()).collect(),
    }
}

fn menu_button() -> gtk::MenuButton {
    let menu = gio::Menu::new();
    menu.append(Some("_Hardware"), Some(&format!("app.{}", parts::ACTION)));
    let status_item = gio::MenuItem::new(Some("_Readings"), None);
    status_item.set_action_and_target_value(
        Some(&format!("app.{}", status::ACTION)),
        Some(&status::target(None)),
    );
    menu.append_item(&status_item);
    menu.append(
        Some("_Preferences"),
        Some(&format!("win.{}", preferences::ACTION)),
    );
    menu.append(Some("_About Frameguin"), Some("app.about"));
    menu.append(Some("_Quit"), Some("app.quit"));
    gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build()
}

pub(crate) fn build_window(
    app: &adw::Application,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
    daemon: Rc<Daemon>,
    feed: Rc<Feed>,
) -> (adw::ApplicationWindow, Rc<Ui>) {
    let battery = battery::Group::build();
    let touchpad = touchpad::Group::build();
    let touchscreen = touchscreen::Group::build();
    let power_led = power_led::Group::build();
    let stack = adw::ViewStack::new();
    let tabs = vec![
        add_tab(
            &stack,
            "Power",
            "battery-symbolic",
            &[&battery.state, &battery.limits],
        ),
        add_tab(
            &stack,
            "Input",
            "input-touchpad-symbolic",
            &[&touchpad.widget, &touchscreen.widget],
        ),
        add_tab(
            &stack,
            "Lights",
            "display-brightness-symbolic",
            &[&power_led.widget],
        ),
    ];

    let preferences = preferences::Group::build();

    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    let header = adw::HeaderBar::new();
    header.pack_end(&menu_button());
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    let empty = fill::build_empty_page(&view, &stack);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Frameguin")
        .default_width(420)
        // Tall enough for the taller tab at the default font scale;
        // re-measure when the rows change.
        .default_height(560)
        .content(&toasts)
        .icon_name(APP_ID)
        .build();

    // Hiding instead of closing only makes sense while a tray icon exists to
    // bring the window back.
    window.set_hide_on_close(tray.is_some());

    let ui = Rc::new(Ui {
        toasts,
        syncing: Cell::new(false),
        header,
        switcher,
        stack,
        tabs,
        battery,
        power_led,
        touchpad,
        touchscreen,
        preferences,
        tray,
        daemon,
        feed,
    });

    ui.preferences.attach(app, &window);
    fill::attach(&window, &ui, empty);

    // On unmap, since switching on map would reload a second tab beside the
    // one the map already reloaded.
    let unmapped_ui = ui.clone();
    window.connect_unmap(move |_| unmapped_ui.show_first_tab());

    (window, ui)
}
