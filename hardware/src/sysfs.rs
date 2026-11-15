//! One attribute of a sysfs directory.

use std::path::Path;

/// Trimmed of the newline sysfs ends an attribute with, and None where the
/// attribute does not read.
pub(crate) fn attribute(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name))
        .ok()
        .map(|value| value.trim().to_owned())
}
