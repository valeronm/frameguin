//! GTK4/libadwaita front-end for Framework laptop controls.
//!
//! Hardware controls go through the frameguin daemon on the system
//! bus; board/BIOS info is read directly from world-readable DMI sysfs.

mod about;
mod autostart;
mod board;
mod caps;
mod format;
mod tray;
mod ui;

use std::cell::RefCell;
use std::ops::ControlFlow;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_wire::FrameguinProxy;
use gtk4::gio;
use gtk4::glib;

use crate::tray::{TrayEvent, TrayIcon, refresh_tray};
use crate::ui::{
    Custom, Sink, Ui, apply_charge_limit, apply_charge_speed, apply_fp_level, build_window,
};

const APP_ID: &str = "io.github.valeronm.Frameguin";

/// The app's one way of attaching to the daemon.
async fn daemon_proxy() -> zbus::Result<FrameguinProxy<'static>> {
    let conn = zbus::Connection::system().await?;
    FrameguinProxy::builder(&conn)
        .destination(frameguin_wire::BUS_NAME)?
        .path(frameguin_wire::OBJECT_PATH)?
        .build()
        .await
}

/// Window and tray state shared between activation and tray events. The
/// window is built on first use, so a service-mode start (autostart) costs
/// only the tray icon.
#[derive(Default)]
struct AppState {
    window: RefCell<Option<(adw::ApplicationWindow, Rc<Ui>)>>,
    tray: RefCell<Option<ksni::blocking::Handle<TrayIcon>>>,
}

impl AppState {
    fn window_for(&self, app: &adw::Application) -> (adw::ApplicationWindow, Rc<Ui>) {
        let mut slot = self.window.borrow_mut();
        slot.get_or_insert_with(|| build_window(app, self.tray.borrow().clone()))
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
        // One connection for the tray's whole life: opening the menu refreshes
        // it, and dialling the bus each time would put a connect and an
        // authentication handshake in front of a menu that has to feel instant.
        let proxy = daemon_proxy().await.ok();
        // Populate the menu right away: in tray-only mode (autostart) nothing
        // else fetches capabilities until the window is first opened, which
        // would leave the menu at Open/Quit.
        if let Some(proxy) = &proxy {
            refresh_tray(&handle, proxy).await;
        }
        while let Ok(event) = rx.recv().await {
            // Where a preset reports: the window when one has been built, the
            // tray itself otherwise. Resolved once, so the fallback is stated
            // in one place however many presets the menu grows.
            let built = state.built_ui();
            let sink = built.as_deref().map_or(Sink::Tray(&handle), Sink::Window);
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
                TrayEvent::Refresh => {
                    if let Some(proxy) = &proxy {
                        refresh_tray(&handle, proxy).await;
                    }
                }
                TrayEvent::SetChargeSpeed(milliamps) => {
                    if let Some(proxy) = &proxy {
                        apply_charge_speed(sink, proxy, milliamps, Custom::Rederive).await;
                    }
                }
                TrayEvent::SetFingerprintLevel(level) => {
                    if let Some(proxy) = &proxy {
                        apply_fp_level(sink, proxy, level).await;
                    }
                }
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
        "Show the version",
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

    app.add_action_entries([
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
    let state = Rc::new(AppState::default());
    let startup_state = state.clone();
    app.connect_startup(move |app| setup_tray(app, startup_state.clone()));
    app.connect_activate(move |app| state.window_for(app).0.present());
    app.run()
}
