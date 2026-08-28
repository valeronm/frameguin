//! GTK4/libadwaita front-end for Framework laptop controls.
//!
//! Hardware controls go through the frameguin daemon on the system
//! bus; board/BIOS info is read directly from world-readable DMI sysfs.

mod about;
mod autostart;
mod battery;
mod board;
mod bus;
mod caps;
mod format;
mod mapped;
mod parts;
mod reading;
mod report;
mod tray;
mod window;

use std::cell::RefCell;
use std::ops::ControlFlow;
use std::rc::Rc;

use adw::prelude::*;
use gtk4::gio;
use gtk4::glib;

use crate::reading::Feed;
use crate::tray::{TrayEvent, TrayIcon, refresh_tray};
use crate::window::{
    Custom, Sink, Ui, apply_charge_limit, apply_charge_speed, apply_power_led_level, build_window,
    touchscreen,
};

const APP_ID: &str = "io.github.valeronm.Frameguin";

/// Window and tray state shared between activation and tray events. The
/// window is built on first use, so a service-mode start (autostart) costs
/// only the tray icon.
#[derive(Default)]
struct AppState {
    window: RefCell<Option<(adw::ApplicationWindow, Rc<Ui>)>>,
    tray: RefCell<Option<ksni::blocking::Handle<TrayIcon>>>,
    /// The pack's reading, taken once for however many windows show it. Here
    /// because it belongs to neither of them: the report can be open with no
    /// window built, and the window outlives any report.
    feed: Rc<Feed>,
}

impl AppState {
    fn window_for(&self, app: &adw::Application) -> (adw::ApplicationWindow, Rc<Ui>) {
        let mut slot = self.window.borrow_mut();
        slot.get_or_insert_with(|| build_window(app, self.tray.borrow().clone(), self.feed.clone()))
            .clone()
    }

    /// The window's widgets if one has been built, without building one. A
    /// tray preset needs somewhere to report, not a window.
    fn built_ui(&self) -> Option<Rc<Ui>> {
        self.window.borrow().as_ref().map(|(_, ui)| ui.clone())
    }
}

fn setup_tray(app: &adw::Application, state: Rc<AppState>) {
    use ksni::blocking::TrayMethods;

    let (tx, rx) = async_channel::unbounded();
    let handle = match TrayIcon::new(tx).spawn() {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("tray icon unavailable: {e}");
            return;
        }
    };
    *state.tray.borrow_mut() = Some(handle.clone());

    let hold = app.hold();
    let app = app.clone();
    glib::spawn_future_local(async move {
        let _hold = hold;
        // Populate the menu right away: in tray-only mode (autostart) nothing
        // else fetches capabilities until the window is first opened, which
        // would leave the menu at Open/Quit.
        refresh_tray(&handle, &state.feed).await;
        while let Ok(event) = rx.recv().await {
            // Where a preset reports: the window when one has been built, the
            // tray itself otherwise. Resolved once, so the fallback is stated
            // in one place however many presets the menu grows.
            let built = state.built_ui();
            let sink = built.as_deref().map_or(Sink::Tray(&handle), Sink::Window);
            // Asked for per write rather than held from startup. The feed
            // keeps the connection once it has one, so this costs a borrow and
            // no handshake — and it costs nothing at all to a session whose
            // first dial failed, where a held `Option` would have swallowed
            // every preset click for as long as the tray ran while the menu
            // beside it went on refreshing.
            let proxy = state.feed.proxy().await.ok();
            let controls = state.feed.probe().await.ok();
            match event {
                TrayEvent::Show => {
                    let window = state.window_for(&app).0;
                    window.unminimize();
                    // A tray event has no activation token, so raising an
                    // already-mapped window is denied by Wayland focus-
                    // stealing prevention. Remapping sidesteps that: a
                    // freshly mapped window is granted focus by the
                    // compositor's new-window policy. Costs the remembered
                    // window position, which Wayland doesn't keep anyway.
                    if window.is_visible() && !window.is_active() {
                        window.set_visible(false);
                    }
                    window.present();
                }
                // Tray presets call the shared write rather than writing by
                // moving the widget: a widget already showing the requested
                // value emits no change, and the click would be swallowed.
                TrayEvent::SetChargeLimit(percent) => {
                    if let Some(proxy) = &proxy {
                        apply_charge_limit(sink, proxy, percent, Custom::Rederive).await;
                    }
                }
                TrayEvent::Refresh => refresh_tray(&handle, &state.feed).await,
                TrayEvent::SetChargeSpeed(milliamps) => {
                    if let Some(proxy) = &proxy {
                        apply_charge_speed(sink, proxy, milliamps, Custom::Rederive).await;
                    }
                }
                TrayEvent::SetPowerLedLevel(level) => {
                    if let Some(proxy) = &proxy {
                        apply_power_led_level(sink, proxy, level).await;
                    }
                }
                TrayEvent::SetTouchscreen(enabled) => {
                    if let Some(control) = controls
                        .as_ref()
                        .and_then(|probe| probe.controls.touchscreen.as_ref())
                    {
                        touchscreen::apply(sink, control, enabled).await;
                    }
                }
                // Through the action, like the window's row: the report has
                // one way in, and a caller reaching past it is how two front-
                // ends come to open a window differently.
                TrayEvent::ShowBatteryDetails => app.activate_action(battery::ACTION, None),
                TrayEvent::Quit => app.quit(),
            }
        }
    });
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();

    // The line is parsed, not just read: it must end in the bare version.
    app.add_main_option(
        "version",
        b'V'.into(),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Print the version",
        None,
    );
    // The About window's report, for the no-display and window-won't-open
    // cases that produce bug reports in the first place.
    app.add_main_option(
        "debug-info",
        glib::Char::from(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Print a hardware report",
        None,
    );
    app.connect_handle_local_options(|_, options| {
        let report = if options.contains("version") {
            format!("frameguin {}\n", env!("CARGO_PKG_VERSION"))
        } else if options.contains("debug-info") {
            glib::MainContext::default().block_on(about::debug_info())
        } else {
            return ControlFlow::Continue(());
        };
        print!("{report}");
        ControlFlow::Break(glib::ExitCode::SUCCESS)
    });

    // Built before the actions rather than beside the handlers below: the
    // report's action needs the feed it holds.
    let state = Rc::new(AppState::default());

    app.add_action_entries([
        // An action rather than a handler on either caller: the window's
        // status row and the tray's reading both open the report, and only an
        // action reaches it from the tray, which builds no widgets and holds
        // no window. The entry is the report's own, so nothing here can reach
        // past it to the window it opens.
        battery::action(state.feed.clone()),
        parts::action(state.feed.clone()),
        gio::ActionEntry::builder("about")
            .activate(|app: &adw::Application, _, _| about::show(app.active_window().as_ref()))
            .build(),
        gio::ActionEntry::builder("quit")
            .activate(|app: &adw::Application, _, _| app.quit())
            .build(),
    ]);

    // Autostart launches with GIO's built-in --gapplication-service, which
    // registers the primary instance without emitting activate — so login
    // brings up only the tray; the first Show or plain launch builds the
    // window.
    let startup_state = state.clone();
    app.connect_startup(move |app| setup_tray(app, startup_state.clone()));
    app.connect_activate(move |app| state.window_for(app).0.present());
    app.run()
}
