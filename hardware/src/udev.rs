//! What udev knows about a device: the properties it cached while processing
//! one, and the vendor list it matched them against. Missing where udevd
//! never ran or the package is not installed, which is no name rather than
//! an error.

use std::fs;

const DATA: &str = "/run/udev/data";

/// The vendor lists as udev ships them, rather than the trie they compile
/// to: the text carries the same data and costs no query tool.
const ACPI_VENDORS: &str = "/usr/lib/udev/hwdb.d/20-acpi-vendor.hwdb";
const USB_VENDORS: &str = "/usr/lib/udev/hwdb.d/20-usb-vendor-model.hwdb";

const VENDOR_NAME: &str = "ID_VENDOR_FROM_DATABASE";
const MODEL_NAME: &str = "ID_MODEL_FROM_DATABASE";

/// What the hardware database called a PCI function, each name empty where
/// nothing gave it one.
pub(crate) struct PciNames {
    pub(crate) vendor: String,
    pub(crate) model: String,
}

/// The names held for the PCI function at `address`.
///
/// The model is cut at the first slash, the database joining a part's
/// alternative names with one.
pub(crate) fn pci_names(address: &str) -> PciNames {
    let record = fs::read_to_string(format!("{DATA}/+pci:{address}")).unwrap_or_default();
    PciNames {
        vendor: property(&record, VENDOR_NAME)
            .unwrap_or_default()
            .to_owned(),
        model: property(&record, MODEL_NAME)
            .map(|model| model.split_once('/').map_or(model, |(first, _)| first))
            .unwrap_or_default()
            .to_owned(),
    }
}

/// Who holds the three-letter PNP id `pnp`, and None where the list does not
/// carry it.
///
/// Read from the list rather than from the device's own record, a panel
/// reached over DRM having the GPU's record and not one of its own.
pub(crate) fn acpi_vendor(pnp: &str) -> Option<String> {
    let listed = fs::read_to_string(ACPI_VENDORS).ok()?;
    vendor(&listed, &format!("acpi:{pnp}*:")).map(str::to_owned)
}

/// Who holds the USB-IF vendor id `vid`, and None where the list does not
/// carry it.
///
/// The id a HID descriptor announces, not the one the bus the device sits on
/// does: a pad on I2C is registered under an ACPI id whose prefix belongs to
/// another company entirely.
pub(crate) fn usb_vendor(vid: u16) -> Option<String> {
    let listed = fs::read_to_string(USB_VENDORS).ok()?;
    vendor(&listed, &format!("usb:v{vid:04X}*")).map(str::to_owned)
}

/// A record line carries its property behind an `E:`.
fn property<'a>(record: &'a str, key: &str) -> Option<&'a str> {
    record
        .lines()
        .filter_map(|line| line.strip_prefix("E:"))
        .find_map(|property| assignment(property, key))
}

/// A list entry is its match line, then the properties indented under it.
fn vendor<'a>(listed: &'a str, key: &str) -> Option<&'a str> {
    listed
        .lines()
        .skip_while(|line| *line != key)
        .skip(1)
        .take_while(|line| line.starts_with(' '))
        .find_map(|line| assignment(line.trim_start(), VENDOR_NAME))
}

fn assignment<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let (name, value) = text.split_once('=')?;
    (name == key).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::{property, vendor};

    const RECORD: &str = "I:2786914\n\
        E:ID_PCI_CLASS_FROM_DATABASE=Mass storage controller\n\
        E:ID_VENDOR_FROM_DATABASE=Sandisk Corp\n\
        E:ID_PATH=pci-0000:01:00.0\n\
        V:1\n";

    const LISTED: &str = "# With various additions from other sources\n\
        \n\
        acpi:CSTI*:\n\
        \x20ID_VENDOR_FROM_DATABASE=CSTI Inc\n\
        \n\
        acpi:CSW*:\n\
        \x20ID_VENDOR_FROM_DATABASE=China Star Optoelectronics Technology Co., Ltd\n\
        \n\
        acpi:CTX*:\n\
        \x20ID_VENDOR_FROM_DATABASE=Cortex\n\
        \n\
        usb:v093A*\n\
        \x20ID_VENDOR_FROM_DATABASE=Pixart Imaging, Inc.\n\
        \n\
        usb:v093Ap0007*\n\
        \x20ID_MODEL_FROM_DATABASE=Optical Mouse\n";

    #[test]
    fn a_property_is_read_off_its_line() {
        assert_eq!(
            property(RECORD, "ID_VENDOR_FROM_DATABASE"),
            Some("Sandisk Corp")
        );
        assert_eq!(property(RECORD, "ID_PATH"), Some("pci-0000:01:00.0"));
    }

    #[test]
    fn a_key_the_record_does_not_carry_is_none() {
        assert!(property(RECORD, "ID_MODEL_FROM_DATABASE").is_none());
        assert!(property("", "ID_VENDOR_FROM_DATABASE").is_none());
    }

    #[test]
    fn a_listed_id_is_named_by_the_entry_indented_under_it() {
        assert_eq!(
            vendor(LISTED, "acpi:CSW*:"),
            Some("China Star Optoelectronics Technology Co., Ltd")
        );
        assert_eq!(vendor(LISTED, "acpi:CTX*:"), Some("Cortex"));
    }

    #[test]
    fn an_id_the_list_does_not_carry_is_none() {
        assert!(vendor(LISTED, "acpi:AUO*:").is_none());
        assert!(vendor("", "acpi:CSW*:").is_none());
    }

    #[test]
    fn an_entry_naming_only_a_model_names_no_vendor() {
        assert_eq!(vendor(LISTED, "usb:v093A*"), Some("Pixart Imaging, Inc."));
        assert!(vendor(LISTED, "usb:v093Ap0007*").is_none());
    }
}
