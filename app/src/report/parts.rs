//! The parts window: the machine's bill of materials, as the daemon detected
//! it — one group per part, its identity and firmware as rows.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::part::{catalogue, kind_label, maker, ordered};
use frameguin_wire::Identity;
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

use super::{Shell, value};
use crate::daemon::Daemon;

/// The application action that opens the window, and the only way in.
pub(crate) const ACTION: &str = "parts";

pub(super) fn action(daemon: Rc<Daemon>) -> gio::ActionEntry<adw::Application> {
    super::action(ACTION, "Parts", 600, move |shell| build(shell, &daemon))
}

/// The rows, fetched once the window is up: the inventory is fixed for the
/// daemon's run and costs one call.
fn build(shell: Shell, daemon: &Rc<Daemon>) {
    let daemon = daemon.clone();
    glib::spawn_future_local(async move {
        let parts = match daemon.bus().await {
            Ok(bus) => bus.frameguin.get_devices().await,
            Err(e) => Err(e),
        };
        match parts {
            Ok(parts) => fill(&shell.page, &parts),
            Err(e) => shell.toast_error("Reading the parts", e),
        }
    });
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
/// catalogue has them and the hardware's own otherwise, then its serial and
/// one row per firmware. A row with nothing to say is left out rather than
/// filled with a placeholder — an I2C-HID descriptor carries no vendor and
/// often no serial, and a column of "Unknown" is what teaches a reader to
/// skip the column.
fn group(part: &Identity) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(kind_label(part.kind))
        .build();
    let sold = catalogue(part);
    if let Some(sold) = sold {
        optional_value(&group, "Model", sold.model);
        if let Some((manufacturer, part_number)) = maker(part) {
            optional_value(&group, "Manufacturer", manufacturer);
            optional_value(&group, "Part number", part_number);
        }
    } else {
        optional_value(&group, "Vendor", &part.vendor);
        optional_value(&group, "Model", &part.model);
    }
    optional_value(&group, "Serial", &part.serial);
    for firmware in &part.firmware {
        optional_value(&group, &firmware.name, &firmware.version);
    }
    if let Some(url) = sold.and_then(|sold| sold.url) {
        group.add(&link_row("Framework Marketplace", url));
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

/// An empty value gets no row.
fn optional_value(group: &adw::PreferencesGroup, title: &str, text: &str) {
    if !text.is_empty() {
        value(group, title).set_label(text);
    }
}
