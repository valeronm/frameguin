//! The shell every read-only window shares — the battery report, the parts
//! list: found by name in the application's own window list or built, a
//! page under a header bar inside a toast overlay, and the row that names
//! one value.
//!
//! Such a window is destroyed on close, unlike the main window, which hides
//! to the tray: a hidden window stays registered with the application and
//! would hold a tray-less app alive with nothing on screen. Nothing is lost
//! by rebuilding — what it reads over belongs to the feed, which outlives
//! any one window — and a second open finds the first by its name rather
//! than in a slot a closed window would leave stale.

use adw::prelude::*;
use gtk4 as gtk;

/// Brings the window to the screen: the one already open where there is
/// one, `build`'s otherwise.
pub(crate) fn present(app: &adw::Application, name: &str, build: impl FnOnce() -> adw::Window) {
    if let Some(open) = app
        .windows()
        .into_iter()
        .find(|window| window.widget_name() == name)
    {
        open.present();
        return;
    }
    build().present();
}

pub(crate) struct Shell {
    pub(crate) window: adw::Window,
    pub(crate) page: adw::PreferencesPage,
    pub(crate) toasts: adw::ToastOverlay,
}

/// `height` is what fits every group at the default font scale; re-measure
/// when the rows change.
pub(crate) fn shell(app: &adw::Application, name: &str, title: &str, height: i32) -> Shell {
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
    window.set_widget_name(name);
    Shell {
        window,
        page,
        toasts,
    }
}

/// A row naming one value, and the label that carries it. Selectable,
/// because a serial or a part number is what a reader came to copy; and
/// wrapping only where the row cannot fit the line — a wrapping label asks
/// for a few characters' width and gets it, the title beside it being what
/// expands, unless told to ask for its whole line.
pub(crate) fn value_row(
    group: &adw::PreferencesGroup,
    title: &str,
) -> (adw::ActionRow, gtk::Label) {
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
