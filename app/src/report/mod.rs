//! The windows that only read — the battery report, the parts list — and
//! the shell they share: a page under a header bar inside a toast overlay,
//! the row that names one value, and the one way such a window is opened.
//!
//! Such a window is destroyed on close, unlike the main window, which hides
//! to the tray: a hidden window stays registered with the application and
//! would hold a tray-less app alive with nothing on screen. Nothing is lost
//! by rebuilding — what it reads over belongs to the daemon handle and the
//! feed, which outlive any one window — and a second open finds the first
//! by its name rather than in a slot a closed window would leave stale.

pub(crate) mod battery;
pub(crate) mod parts;

use std::rc::Rc;

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::gio;

use frameguin_wire::DeviceError;

use crate::daemon::Daemon;
use crate::reading::Feed;

/// Every report's action, for the application to register together. An
/// action rather than a handler on either caller because the tray, which
/// builds no widgets and holds no window, opens the battery report too; and
/// one list, because a report is reachable only through its action, so one
/// left out would be a menu row that does nothing.
pub(crate) fn actions(
    daemon: &Rc<Daemon>,
    feed: &Rc<Feed>,
) -> [gio::ActionEntry<adw::Application>; 2] {
    [
        battery::action(daemon.clone(), feed.clone()),
        parts::action(daemon.clone()),
    ]
}

/// The shell one report fills, and where it says a read failed.
struct Shell {
    page: adw::PreferencesPage,
    toasts: adw::ToastOverlay,
}

impl Shell {
    /// A read that failed, named by what was being attempted — "Reading the
    /// battery". Built here so every report loses the D-Bus error name the
    /// same way, whether it holds a bus error or a `DeviceError`.
    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        let message = format!("{attempt} failed: {}", error.into());
        self.toasts.add_toast(adw::Toast::new(&message));
    }
}

/// The action that opens one report: the window already open for it where
/// there is one, and otherwise a fresh shell handed to `fill` and presented.
/// The one way in, so a report cannot build a window the lookup would not
/// find — the window is named after the action, and only reports name one.
///
/// `height` is what fits every group at the default font scale; re-measure
/// when the rows change.
fn action(
    action: &'static str,
    title: &'static str,
    height: i32,
    fill: impl Fn(Shell) + 'static,
) -> gio::ActionEntry<adw::Application> {
    gio::ActionEntry::builder(action)
        .activate(move |app: &adw::Application, _, _| {
            if let Some(open) = app
                .windows()
                .into_iter()
                .find(|window| window.widget_name() == action)
            {
                open.present();
                return;
            }
            let page = adw::PreferencesPage::new();
            let view = adw::ToolbarView::new();
            view.add_top_bar(&adw::HeaderBar::new());
            view.set_content(Some(&page));
            let toasts = adw::ToastOverlay::new();
            toasts.set_child(Some(&view));
            let window = adw::Window::builder()
                .application(app)
                .title(title)
                .default_width(420)
                .default_height(height)
                .content(&toasts)
                .build();
            window.set_widget_name(action);
            fill(Shell { page, toasts });
            window.present();
        })
        .build()
}

/// A row naming one value, and the label that carries it. Selectable,
/// because a serial or a part number is what a reader came to copy; and
/// wrapping only where the row cannot fit the line — a wrapping label asks
/// for a few characters' width and gets it, the title beside it being what
/// expands, unless told to ask for its whole line.
fn value_row(group: &adw::PreferencesGroup, title: &str) -> (adw::ActionRow, gtk::Label) {
    let row = adw::ActionRow::builder().title(title).build();
    let value = gtk::Label::builder()
        .selectable(true)
        .wrap(true)
        .natural_wrap_mode(gtk::NaturalWrapMode::None)
        .xalign(1.0)
        .build();
    value.add_css_class("dim-label");
    row.add_suffix(&value);
    group.add(&row);
    (row, value)
}

/// A row whose value is all anyone needs back — most of them. The row itself
/// is worth keeping only where something moves it later, hence
/// [`value_row`] beside this.
fn value(group: &adw::PreferencesGroup, title: &str) -> gtk::Label {
    value_row(group, title).1
}
