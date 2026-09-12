//! udev's record of a device: what it resolved while processing one, absent
//! where udevd never ran.

use std::fs;

const DATA: &str = "/run/udev/data";

/// What the hardware database called the maker of the PCI function at
/// `address`, and None where nothing named it.
pub(crate) fn pci_vendor(address: &str) -> Option<String> {
    let record = fs::read_to_string(format!("{DATA}/+pci:{address}")).ok()?;
    property(&record, "ID_VENDOR_FROM_DATABASE").map(str::to_owned)
}

/// A record line carries its property behind an `E:`.
fn property<'a>(record: &'a str, key: &str) -> Option<&'a str> {
    record
        .lines()
        .filter_map(|line| line.strip_prefix("E:"))
        .filter_map(|property| property.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::property;

    const RECORD: &str = "I:2786914\n\
        E:ID_PCI_CLASS_FROM_DATABASE=Mass storage controller\n\
        E:ID_VENDOR_FROM_DATABASE=Sandisk Corp\n\
        E:ID_PATH=pci-0000:01:00.0\n\
        V:1\n";

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
}
