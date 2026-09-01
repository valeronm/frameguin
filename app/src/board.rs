//! What the machine says about itself, read straight from world-readable DMI
//! sysfs. The one hardware fact the app learns without asking the daemon, so
//! the header can name a board before — or without — the bus answering.

use std::fs;
use std::sync::OnceLock;

pub(crate) fn dmi(file: &str) -> String {
    fs::read_to_string(format!("/sys/class/dmi/id/{file}"))
        .map_or_else(|_| "unknown".into(), |s| s.trim().to_string())
}

/// The board's name, and None on anything that isn't a Framework. The same
/// vendor test the daemon gates its EC on — spelled in `wire` so the two
/// cannot come to different answers about whether there is hardware here to
/// control.
///
/// Read once and kept: the machine cannot become another one under a running
/// app, and a caller on a timer would otherwise re-read two sysfs files every
/// tick to be told the same thing.
pub(crate) fn detected() -> Option<&'static str> {
    static DETECTED: OnceLock<Option<String>> = OnceLock::new();
    DETECTED
        .get_or_init(|| (dmi("sys_vendor") == frameguin_wire::VENDOR).then(|| dmi("product_name")))
        .as_deref()
}

/// The board's name for a caller that has no second thing to say about a
/// machine that is not a Framework — every table keyed on the product name
/// matches nothing for the empty string, so the absence needs no branch.
/// Spelled here rather than at each such caller, which would be that
/// reasoning re-decided per site.
pub(crate) fn product() -> &'static str {
    detected().unwrap_or_default()
}
