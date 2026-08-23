//! What the machine says about itself, read straight from world-readable DMI
//! sysfs. The one hardware fact the app learns without asking the daemon, so
//! the header can name a board before — or without — the bus answering.

use std::fs;

pub(crate) fn dmi(file: &str) -> String {
    fs::read_to_string(format!("/sys/class/dmi/id/{file}"))
        .map_or_else(|_| "unknown".into(), |s| s.trim().to_string())
}

/// The board's name, and None on anything that isn't a Framework. The same
/// vendor test the daemon gates its EC on, so the two can't disagree about
/// whether there is hardware here to control.
pub(crate) fn detected() -> Option<String> {
    (dmi("sys_vendor") == "Framework").then(|| dmi("product_name"))
}
