//! The Preferences dialog: the rows about the app rather than the hardware,
//! opened from the main window's `win.preferences` action, and their writes.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_wire::DeviceError;
use gtk4::gio;

use super::Ui;
use super::widgets::{connect_switch, show_switch};
use crate::{autostart, failure};

pub(super) const ACTION: &str = "preferences";

/// Read off the row rather than spelled again, because a message naming a
/// row by a title it no longer has is worse than a vaguer one.
fn setting(row: &adw::SwitchRow) -> String {
    format!("Setting “{}”", row.title())
}

/// Kept for the window's life rather than built per open, its rows being
/// read and connected once with the window.
pub(super) struct Group {
    dialog: adw::PreferencesDialog,
    autostart: adw::SwitchRow,
    /// The daemon's restore switch, which is no control: it belongs to no
    /// device and is set through the root interface.
    restore: adw::SwitchRow,
}

impl Group {
    pub(super) fn build() -> Self {
        let autostart_row = adw::SwitchRow::builder()
            .title("Start at login")
            .subtitle("Show only the tray icon until opened")
            .active(autostart::entry_path().exists())
            .build();
        let restore = adw::SwitchRow::builder()
            .title("Restore settings")
            .subtitle("Put back what was set here after a restart or a resume")
            .sensitive(false)
            .build();
        let group = adw::PreferencesGroup::new();
        group.add(&autostart_row);
        group.add(&restore);
        let page = adw::PreferencesPage::new();
        page.add(&group);
        let dialog = adw::PreferencesDialog::new();
        dialog.add(&page);
        Self {
            dialog,
            autostart: autostart_row,
            restore,
        }
    }

    /// Where a row's failure is told: the dialog is drawn over the window's
    /// own toasts.
    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        self.dialog.add_toast(failure::toast(attempt, error));
    }

    /// Registers the action that opens the dialog, and the autostart row's
    /// write, which needs no daemon.
    pub(super) fn attach(&self, app: &adw::Application, window: &adw::ApplicationWindow) {
        let dialog = self.dialog.clone();
        let open = gio::ActionEntry::builder(ACTION)
            .activate(move |window: &adw::ApplicationWindow, _, _| {
                dialog.present(Some(window));
            })
            .build();
        window.add_action_entries([open]);
        app.set_accels_for_action(&format!("win.{ACTION}"), &["<Control>comma"]);

        let dialog = self.dialog.clone();
        self.autostart.connect_active_notify(move |row| {
            if let Err(e) = autostart::set(row.is_active()) {
                dialog.add_toast(failure::toast(&setting(row), e));
            }
        });
    }

    /// Read once, with the first fill: outside debugging, this switch is the
    /// only writer.
    pub(super) async fn load(&self, ui: &Ui) {
        match async { ui.daemon.bus().await?.frameguin.get_restore().await }.await {
            Ok(enabled) => show_switch(ui, &self.restore, enabled),
            Err(e) => ui.toast_error(&format!("Reading “{}”", self.restore.title()), e),
        }
    }

    /// A refused write leaves the switch claiming a setting will be there
    /// after a restart, and its prior value is the negation.
    pub(super) fn connect(&self, ui: &Rc<Ui>) {
        connect_switch(
            ui,
            &ui.daemon,
            &self.restore,
            |ui, daemon, enabled| async move {
                let written =
                    async { daemon.bus().await?.frameguin.set_restore(enabled).await }.await;
                if let Err(e) = written {
                    let group = &ui.preferences;
                    group.toast_error(&setting(&group.restore), e);
                    show_switch(&ui, &group.restore, !enabled);
                }
            },
        );
    }
}
