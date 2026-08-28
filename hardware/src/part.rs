//! What a detected device is, as a part of the machine — the facet a bill of
//! materials iterates, asked through one trait because its caller does not
//! care what any entry does.

/// What kind of part this is, named for the thing a person would buy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Memory,
    Touchpad,
}

/// What detection saw, kept as it was announced: the words are the
/// hardware's own, and any mapping to the part a person buys is a table
/// keyed on `id`, kept where the words are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub kind: Kind,
    pub vendor: String,
    pub model: String,
    pub serial: Option<String>,
    /// The identifier the part announces itself by, prefixed with the space
    /// it is drawn from — `usb:093a:1343`, `dmi-slot:LPCAMM2_0`.
    pub id: String,
}

pub trait Part {
    fn identity(&self) -> &Identity;
}
