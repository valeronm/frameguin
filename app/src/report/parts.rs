//! The parts window: the machine's bill of materials, as the daemon detected
//! it — the parts in a sidebar, the selected one's identity and firmware as
//! rows beside it.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::part::{catalogue, inventory, maker, name, part_number};
use frameguin_wire::Identity;
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

use super::{Shell, value};
use crate::daemon::Daemon;

/// The application action that opens the window, and the only way in.
pub(crate) const ACTION: &str = "parts";

const TITLE: &str = "Parts";

pub(super) fn action(daemon: Rc<Daemon>) -> gio::ActionEntry<adw::Application> {
    super::entry(ACTION, TITLE, (760, 560), move |shell, window| {
        build(shell, window, &daemon)
    })
}

/// The panes, filled once the daemon answers: the inventory is fixed for the
/// daemon's run and costs one call.
fn build(shell: Shell, window: &adw::Window, daemon: &Rc<Daemon>) -> adw::NavigationSplitView {
    let panes = panes();
    let split = panes.split.clone();
    collapse_when_narrow(window, &split);
    let daemon = daemon.clone();
    glib::spawn_future_local(async move {
        let parts = match daemon.bus().await {
            Ok(bus) => bus.frameguin.get_devices().await,
            Err(e) => Err(e),
        };
        match parts {
            // A window closed while the daemon was being dialled has nothing
            // left to draw into.
            Ok(parts) if panes.split.root().is_some() => panes.fill(&parts),
            Ok(_) => (),
            Err(e) => shell.toast_error("Reading the parts", e),
        }
    });
    split
}

/// The panes stand empty until the daemon answers.
struct Panes {
    split: adw::NavigationSplitView,
    list: gtk::ListBox,
    pages: gtk::Stack,
    titled: adw::NavigationPage,
}

fn panes() -> Panes {
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    let listed = super::headed(&gtk::ScrolledWindow::builder().child(&list).build());
    let pages = gtk::Stack::new();
    // A homogeneous stack measures every part's page on every layout pass,
    // where only the page on screen has anything to say about the pane's
    // size.
    pages.set_hhomogeneous(false);
    pages.set_vhomogeneous(false);
    let titled = adw::NavigationPage::builder()
        .title(TITLE)
        .child(&super::headed(&pages))
        .build();
    let sidebar = adw::NavigationPage::builder()
        .title(TITLE)
        .child(&listed)
        .build();
    let split = adw::NavigationSplitView::builder()
        .sidebar(&sidebar)
        .content(&titled)
        .build();
    Panes {
        split,
        list,
        pages,
        titled,
    }
}

impl Panes {
    /// A content pane showing nothing reads as a window that failed, so the
    /// first part is selected.
    fn fill(self, parts: &[Identity]) {
        let Panes {
            split,
            list,
            pages,
            titled,
        } = self;
        let rows: Vec<(String, adw::PreferencesPage)> = inventory(parts)
            .into_iter()
            .map(|(part, title)| (title, details(part)))
            .collect();
        if rows.is_empty() {
            pages.add_child(
                &adw::StatusPage::builder()
                    .icon_name("dialog-information-symbolic")
                    .title("No parts detected")
                    .description("The daemon found no parts on this machine.")
                    .build(),
            );
            return;
        }
        for (title, page) in &rows {
            list.append(&adw::ActionRow::builder().title(title).build());
            pages.add_child(page);
        }
        // Showing the content is for a selection the reader made, not for
        // the one a fill starts with.
        titled.set_title(&rows[0].0);
        list.select_row(list.row_at_index(0).as_ref());
        // The list is the split view's own descendant, so a handler holding
        // it strongly is the window keeping itself alive past its close.
        let split = split.downgrade();
        list.connect_row_selected(move |_, row| {
            let Some((title, page)) = row
                .and_then(|row| usize::try_from(row.index()).ok())
                .and_then(|index| rows.get(index))
            else {
                return;
            };
            pages.set_visible_child(page);
            titled.set_title(title);
            if let Some(split) = split.upgrade() {
                split.set_show_content(true);
            }
        });
    }
}

/// Below this the panes do not both fit, so the sidebar becomes a page the
/// details are navigated back to.
fn collapse_when_narrow(window: &adw::Window, split: &adw::NavigationSplitView) {
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        500.0,
        adw::LengthUnit::Px,
    ));
    narrow.add_setter(split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(narrow);
}

/// A row with nothing to say is left out rather than filled with a
/// placeholder — an I2C-HID descriptor carries no vendor and often no
/// serial, and a column of "Unknown" is what teaches a reader to skip the
/// column.
fn details(part: &Identity) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let sold = catalogue(part);
    let group = adw::PreferencesGroup::builder()
        .title(name(part, sold))
        .build();
    if let Some(sold) = sold {
        group.set_description(sold.variant);
        if let Some(url) = sold.url {
            group.set_header_suffix(Some(&link_button(url)));
        }
    }
    optional_value(&group, "Manufacturer", maker(part));
    optional_value(&group, "Part number", part_number(part, sold));
    optional_value(&group, "Serial", &part.serial);
    for firmware in &part.firmware {
        optional_value(&group, &firmware.name, &firmware.version);
    }
    page.add(&group);
    page
}

/// A button that opens a page, drawn as the About window draws its links:
/// the external-link icon libadwaita ships for exactly that.
fn link_button(url: &'static str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name("adw-external-link-symbolic")
        .tooltip_text("Framework Marketplace")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    button.connect_clicked(move |_| {
        let _ = gio::AppInfo::launch_default_for_uri(url, gio::AppLaunchContext::NONE);
    });
    button
}

/// An empty value gets no row.
fn optional_value(group: &adw::PreferencesGroup, title: &str, text: &str) {
    if !text.is_empty() {
        value(group, title).set_label(text);
    }
}
