//! What a detected device is, as a part of the machine — the facet a bill of
//! materials iterates, asked through one trait because its caller does not
//! care what any entry does.

pub use frameguin_wire::{Firmware, Identity, PartKind};

/// A HID part, from what its descriptor announces. The ids are the USB-IF
/// registry's, but the space is HID's: the same ids arrive over I2C, and the
/// part is the same part whichever bus carries it.
pub fn hid(
    kind: PartKind,
    vid: u16,
    pid: u16,
    manufacturer: &str,
    product: &str,
    serial: &str,
) -> Identity {
    Identity {
        kind,
        vendor: manufacturer.to_owned(),
        model: product.to_owned(),
        part_number: String::new(),
        serial: serial.to_owned(),
        id: format!("hid:{vid:04x}:{pid:04x}"),
        firmware: Vec::new(),
    }
}

/// [`hid`] read off an enumerated device. An absent string is a descriptor
/// that carries none, kept empty rather than guessed.
pub fn of_hid(kind: PartKind, dev: &hidapi::DeviceInfo) -> Identity {
    hid(
        kind,
        dev.vendor_id(),
        dev.product_id(),
        dev.manufacturer_string().unwrap_or_default(),
        dev.product_string().unwrap_or_default(),
        dev.serial_number().unwrap_or_default(),
    )
}

/// A panel, from what its EDID announces. The PNP id and the product code
/// are the two things every EDID carries, so the identifier is those where
/// the optional name descriptor leaves the model empty.
pub fn edid(manufacturer: &str, product: u16, name: &str, serial: &str) -> Identity {
    Identity {
        kind: PartKind::Display,
        vendor: manufacturer.to_owned(),
        model: name.to_owned(),
        part_number: String::new(),
        serial: serial.to_owned(),
        id: format!("edid:{manufacturer}:{product:04x}"),
        firmware: Vec::new(),
    }
}

/// A Smart Battery, from what its gauge announces over the EC. The model
/// number is the part's rather than the unit's; the serial is the unit's.
pub fn sbs(manufacturer: &str, model: &str, serial: &str) -> Identity {
    Identity {
        kind: PartKind::Battery,
        vendor: manufacturer.to_owned(),
        model: model.to_owned(),
        part_number: String::new(),
        serial: serial.to_owned(),
        id: format!("sbs:{model}"),
        firmware: Vec::new(),
    }
}

pub trait Part {
    fn identity(&self) -> &Identity;
}
