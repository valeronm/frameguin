//! The windows that only read — the battery report, the parts list — and
//! the shell they share: a toast overlay filling the window, the page under
//! a header bar most of them are drawn as, the row that names one value,
//! and the one way such a window is opened.
//!
//! Such a window is destroyed on close, unlike the main window, which hides
//! to the tray: a hidden window stays registered with the application and
//! would hold a tray-less app alive with nothing on screen. Nothing is lost
//! by rebuilding — what it reads over belongs to the daemon handle and the
//! feed, which outlive any one window — and a second open finds the first
//! by its name rather than in a slot a closed window would leave stale.

pub(crate) mod battery;
pub(crate) mod parts;
pub(crate) mod ports;

use std::rc::Rc;

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::gio;

use frameguin_wire::DeviceError;

use crate::daemon::Daemon;
use crate::failure;
use crate::reading::Feed;

/// Every report's action, for the application to register together. An
/// action rather than a handler on either caller because the tray, which
/// builds no widgets and holds no window, opens the battery report too; and
/// one list, because a report is reachable only through its action, so one
/// left out would be a menu row that does nothing.
pub(crate) fn actions(
    daemon: &Rc<Daemon>,
    feed: &Rc<Feed>,
) -> [gio::ActionEntry<adw::Application>; 3] {
    [
        battery::action(daemon.clone(), feed.clone()),
        parts::action(daemon.clone()),
        ports::action(feed.clone()),
    ]
}

/// Where a report says a read failed.
struct Shell {
    toasts: adw::ToastOverlay,
}

impl Shell {
    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        failure::toast(&self.toasts, attempt, error);
    }
}

/// The action that opens one report: the window already open for it where
/// there is one, and otherwise a fresh shell whose content `fill` builds.
/// The one way in, so a report cannot build a window the lookup would not
/// find — the window is named after the action, and only reports name one.
fn entry<W: IsA<gtk::Widget>>(
    action: &'static str,
    title: &'static str,
    size: (i32, i32),
    fill: impl Fn(Shell, &adw::Window) -> W + 'static,
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
            let (width, height) = size;
            let window = adw::Window::builder()
                .application(app)
                .title(title)
                .default_width(width)
                .default_height(height)
                .build();
            window.set_widget_name(action);
            let toasts = adw::ToastOverlay::new();
            let content = fill(
                Shell {
                    toasts: toasts.clone(),
                },
                &window,
            );
            toasts.set_child(Some(&content));
            window.set_content(Some(&toasts));
            window.present();
        })
        .build()
}

/// A report drawn as one page of rows under a header bar.
///
/// `height` is what fits every group at the default font scale; re-measure
/// when the rows change.
fn action(
    action: &'static str,
    title: &'static str,
    height: i32,
    fill: impl Fn(Shell, &adw::PreferencesPage) + 'static,
) -> gio::ActionEntry<adw::Application> {
    entry(action, title, (420, height), move |shell, _| {
        let page = adw::PreferencesPage::new();
        let view = headed(&page);
        fill(shell, &page);
        view
    })
}

/// The window carries no titlebar of its own, so its controls sit in this
/// header bar.
fn headed(content: &impl IsA<gtk::Widget>) -> adw::ToolbarView {
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(content));
    view
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
