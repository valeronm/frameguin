//! What a detected device is, as a part of the machine — the facet a bill of
//! materials iterates, asked through one trait because its caller does not
//! care what any entry does.

/// What kind of part this is, named for the thing a person would buy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Memory,
    Touchpad,
    Touchscreen,
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
    /// The part's firmware version as the vendor spells it, and None where
    /// the part carries none or would not say — a version is never worth a
    /// failed detection.
    pub firmware: Option<String>,
}

impl Identity {
    /// A part on the HID bus, from what its descriptor announces. An empty
    /// string is a descriptor that carries none, kept empty rather than
    /// guessed.
    pub fn usb(
        kind: Kind,
        vid: u16,
        pid: u16,
        manufacturer: &str,
        product: &str,
        serial: &str,
    ) -> Self {
        Self {
            kind,
            vendor: manufacturer.to_owned(),
            model: product.to_owned(),
            serial: (!serial.is_empty()).then(|| serial.to_owned()),
            id: format!("usb:{vid:04x}:{pid:04x}"),
            firmware: None,
        }
    }

    /// [`Identity::usb`] read off an enumerated device.
    pub fn of_hid(kind: Kind, dev: &hidapi::DeviceInfo) -> Self {
        Self::usb(
            kind,
            dev.vendor_id(),
            dev.product_id(),
            dev.manufacturer_string().unwrap_or_default(),
            dev.product_string().unwrap_or_default(),
            dev.serial_number().unwrap_or_default(),
        )
    }
}

pub trait Part {
    fn identity(&self) -> &Identity;
}
