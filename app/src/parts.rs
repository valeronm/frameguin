//! The parts window: the machine's bill of materials, as the daemon detected
//! it — one group per part, its identity and firmware as rows.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::part::{catalogue, kind_label, maker, ordered};
use frameguin_wire::{Identity, cause};
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

use crate::reading::Feed;
use crate::report::{self, Shell};

const WINDOW_NAME: &str = "frameguin-parts";

/// The application action that opens the window, and the only way in.
pub(crate) const ACTION: &str = "parts";

pub(crate) fn action(feed: Rc<Feed>) -> gio::ActionEntry<adw::Application> {
    gio::ActionEntry::builder(ACTION)
        .activate(move |app: &adw::Application, _, _| {
            report::present(app, WINDOW_NAME, || build(app, &feed));
        })
        .build()
}

/// The window, built and left to fill itself: the inventory is fixed for
/// the daemon's run and costs one call to fetch.
fn build(app: &adw::Application, feed: &Rc<Feed>) -> adw::Window {
    let Shell {
        window,
        page,
        toasts,
    } = report::shell(app, WINDOW_NAME, "Parts", 600);

    let feed = feed.clone();
    glib::spawn_future_local(async move {
        let parts = match feed.proxy().await {
            Ok(proxy) => proxy.get_devices().await,
            Err(e) => Err(e),
        };
        match parts {
            Ok(parts) => fill(&page, &parts),
            Err(e) => {
                let message = format!("Reading the parts failed: {}", cause(&e));
                toasts.add_toast(adw::Toast::new(&message));
            }
        }
    });

    window
}

fn fill(page: &adw::PreferencesPage, parts: &[Identity]) {
    if parts.is_empty() {
        let none = adw::PreferencesGroup::builder()
            .description("The daemon found no parts on this machine.")
            .build();
        page.add(&none);
        return;
    }
    for part in ordered(parts) {
        page.add(&group(part));
    }
}

/// One part: its kind as the title, the words it is sold under where the
/// catalogue has them and the hardware's own otherwise, then its serial,
/// identifier and one row per firmware. A row with nothing to say is left
/// out rather than filled with a placeholder — an I2C-HID descriptor
/// carries no vendor and often no serial, and a column of "Unknown" is what
/// teaches a reader to skip the column.
fn group(part: &Identity) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(kind_label(part.kind))
        .build();
    if let Some(sold) = catalogue(part) {
        value_row(&group, "Model", sold.model);
        if let Some((manufacturer, part_number)) = maker(part) {
            value_row(&group, "Manufacturer", manufacturer);
            value_row(&group, "Part number", part_number);
        }
    } else {
        value_row(&group, "Vendor", &part.vendor);
        value_row(&group, "Model", &part.model);
    }
    value_row(&group, "Serial", &part.serial);
    value_row(&group, "Identifier", &part.id);
    for firmware in &part.firmware {
        value_row(&group, &firmware.name, &firmware.version);
    }
    if let Some(sold) = catalogue(part) {
        group.add(&link_row("Framework Marketplace", sold.url));
    }
    group
}

/// A row that opens a page, drawn as the About window draws its links: the
/// title, and the external-link icon libadwaita ships for exactly that.
fn link_row(title: &str, url: &'static str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .activatable(true)
        .build();
    row.add_suffix(&gtk::Image::from_icon_name("adw-external-link-symbolic"));
    row.connect_activated(move |_| {
        let _ = gio::AppInfo::launch_default_for_uri(url, gio::AppLaunchContext::NONE);
    });
    row
}

/// None for an empty value, which gets no row.
fn value_row(group: &adw::PreferencesGroup, title: &str, value: &str) -> Option<adw::ActionRow> {
    if value.is_empty() {
        return None;
    }
    let (row, label) = report::value_row(group, title);
    label.set_label(value);
    Some(row)
}
