//! The Display group: the touchscreen's switch, and the one write both
//! front-ends make through it.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::touchscreen;
use gtk4::glib;

use crate::bus::Bus;
use crate::tray::TrayValues;
use crate::window::{Sink, Ui};

pub(crate) type Touchscreen = touchscreen::Touchscreen<Bus>;

pub(crate) struct Group {
    pub(crate) widget: adw::PreferencesGroup,
    switch: adw::SwitchRow,
}

impl Group {
    pub(crate) fn build() -> Self {
        let widget = adw::PreferencesGroup::builder().title("Display").build();
        let switch = adw::SwitchRow::builder()
            .title("Touchscreen")
            .subtitle("Comes back on at the next lid open, resume or restart")
            .sensitive(false)
            .build();
        widget.add(&switch);
        Self { widget, switch }
    }

    /// Shows the group where the machine has a panel to switch, and hides it
    /// otherwise.
    pub(crate) fn gate(&self, control: Option<&Rc<Touchscreen>>) {
        self.widget.set_visible(control.is_some());
    }

    pub(crate) async fn load(&self, ui: &Ui, control: &Touchscreen, values: &mut TrayValues) {
        match control.read().await {
            Ok(enabled) => {
                self.show(ui, enabled);
                values.touchscreen = Some(enabled);
            }
            Err(e) => ui.toast_error("Reading the touchscreen", e),
        }
    }

    /// Moves the switch without its handler writing it back, and makes it
    /// usable — a row is only ever filled from a read that succeeded.
    fn show(&self, ui: &Ui, enabled: bool) {
        ui.sync(|| {
            self.switch.set_active(enabled);
            self.switch.set_sensitive(true);
        });
    }

    pub(crate) fn connect(&self, ui: &Rc<Ui>, control: &Rc<Touchscreen>) {
        let switch_ui = ui.clone();
        let switch_control = control.clone();
        self.switch.connect_active_notify(move |row| {
            if switch_ui.syncing.get() {
                return;
            }
            let enabled = row.is_active();
            let ui = switch_ui.clone();
            let control = switch_control.clone();
            glib::spawn_future_local(async move {
                apply(Sink::Window(&ui), &control, enabled).await;
            });
        });
    }
}

/// The one write for the touchscreen. The window's switch and the tray's row
/// both come here, so neither can drift from the other on what it reports or
/// what it tells the tray.
///
/// Unlike its siblings this one puts the widget *back* when the write fails.
/// The switch is already carrying the requested value by then — the click is
/// what moved it — and what it would otherwise assert is "touch is off",
/// which someone believes by waiting for a tap that never comes. A refused
/// polkit prompt is the ordinary way to get here, not an exotic one.
pub(crate) async fn apply(sink: Sink<'_>, control: &Touchscreen, enabled: bool) {
    // The tray is told only of a write that landed: its copy never moved,
    // and pushing it back would block this thread while ksni rebuilt the
    // whole menu to merge a value it already holds.
    let standing = match control.set_enabled(enabled).await {
        Ok(()) => {
            sink.push_tray(TrayValues::touchscreen(enabled));
            enabled
        }
        Err(e) => {
            sink.toast_error("Switching the touchscreen", e);
            !enabled
        }
    };
    if let Sink::Window(ui) = sink {
        ui.touchscreen.show(ui, standing);
    }
}
