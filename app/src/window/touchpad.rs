//! The Touchpad group: two combos, each a row per value.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::touchpad::{
    self, Snapshot, click_force_at, click_force_labels, click_force_row, haptic_at, haptic_labels,
    haptic_row,
};
use gtk4::glib;

use crate::bus::Bus;
use crate::window::{Ui, combo_selection, string_list};

pub(crate) type Touchpad = touchpad::Touchpad<Bus>;

pub(crate) struct Group {
    pub(crate) widget: adw::PreferencesGroup,
    haptic_combo: adw::ComboRow,
    force_combo: adw::ComboRow,
}

impl Group {
    pub(crate) fn build() -> Self {
        let widget = adw::PreferencesGroup::builder().title("Touchpad").build();
        let haptic_combo = adw::ComboRow::builder()
            .title("Haptic intensity")
            .subtitle("How strong the click feels")
            .model(&string_list(&haptic_labels()))
            .sensitive(false)
            .build();
        widget.add(&haptic_combo);
        let force_combo = adw::ComboRow::builder()
            .title("Click force")
            .subtitle("How hard you press to click")
            .model(&string_list(&click_force_labels()))
            .sensitive(false)
            .build();
        widget.add(&force_combo);
        Self {
            widget,
            haptic_combo,
            force_combo,
        }
    }

    /// Shows the group where the board has the pad, and hides it otherwise.
    pub(crate) fn gate(&self, control: Option<&Rc<Touchpad>>) {
        self.widget.set_visible(control.is_some());
    }

    /// Moves both combos onto the snapshot without their handlers writing it
    /// back, and makes them usable — a row is only ever filled from a read
    /// that succeeded.
    pub(crate) fn show(&self, ui: &Ui, snapshot: Snapshot) {
        ui.sync(|| {
            self.haptic_combo
                .set_selected(combo_selection(haptic_row(snapshot.haptic_intensity)));
            self.haptic_combo.set_sensitive(true);
            self.force_combo
                .set_selected(combo_selection(click_force_row(snapshot.click_force)));
            self.force_combo.set_sensitive(true);
        });
    }

    /// Nothing moves on success: the combo is already on the row the user
    /// picked, the pad makes no announcement, and the tray has no item for
    /// it. A refusal is toasted and the row left where the click put it — a
    /// stale row here outlives nothing worse than the next reload.
    pub(crate) fn connect(&self, ui: &Rc<Ui>, control: &Rc<Touchpad>) {
        let haptic_ui = ui.clone();
        let haptic_control = control.clone();
        self.haptic_combo.connect_selected_notify(move |row| {
            if haptic_ui.syncing.get() {
                return;
            }
            let Some(percent) = haptic_at(row.selected() as usize) else {
                return;
            };
            let ui = haptic_ui.clone();
            let control = haptic_control.clone();
            glib::spawn_future_local(async move {
                if let Err(e) = control.set_haptic_intensity(percent).await {
                    ui.toast_error("Setting haptic intensity", e);
                }
            });
        });

        let force_ui = ui.clone();
        let force_control = control.clone();
        self.force_combo.connect_selected_notify(move |row| {
            if force_ui.syncing.get() {
                return;
            }
            let Some(force) = click_force_at(row.selected() as usize) else {
                return;
            };
            let ui = force_ui.clone();
            let control = force_control.clone();
            glib::spawn_future_local(async move {
                if let Err(e) = control.set_click_force(force).await {
                    ui.toast_error("Setting click force", e);
                }
            });
        });
    }
}
