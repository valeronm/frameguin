//! Words more than one control shows.

use std::fmt::Display;

/// Nothing is powering the machine — a state two controls reach from
/// different devices, the EC's own flag and the USB-C ports, and one neither
/// of them owns.
pub const NO_SUPPLY: &str = "Not connected";

/// Parts read on one line, separated alike wherever a line lists them.
#[must_use]
pub fn joined<S: AsRef<str>>(parts: impl IntoIterator<Item = S>) -> String {
    let parts: Vec<S> = parts.into_iter().collect();
    parts
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<&str>>()
        .join(" · ")
}

/// A range, its unit written once with `high`.
#[must_use]
pub fn span(low: impl Display, high: impl Display) -> String {
    format!("{low}–{high}")
}

/// A two-state reading, worded alike by every control that shows one.
#[must_use]
pub fn yes_no(set: bool) -> &'static str {
    if set { "Yes" } else { "No" }
}

/// Only a decimal is trimmed: a whole number's trailing zeros are its value.
#[must_use]
pub fn trimmed(mut spelled: String) -> String {
    if spelled.contains('.') {
        spelled.truncate(spelled.trim_end_matches('0').trim_end_matches('.').len());
    }
    spelled
}
