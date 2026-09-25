//! Words more than one control shows.

/// Nothing is powering the machine — a state two controls reach from
/// different devices, the EC's own flag and the USB-C ports, and one neither
/// of them owns.
pub const NO_SUPPLY: &str = "Not connected";

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
