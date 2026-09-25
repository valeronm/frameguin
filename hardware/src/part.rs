//! What a detected device is, as a part of the machine — the facet a bill of
//! materials iterates, asked through one trait because its caller does not
//! care what any entry does.

pub use frameguin_contract::{Detail, Firmware, FirmwareKind, Identity, PartKind};

use crate::udev::PciNames;
use crate::usb::BusDevice;

/// A HID part, from what its descriptor announces. The ids are the USB-IF
/// registry's, but the space is HID's: the same ids arrive over I2C, and the
/// part is the same part whichever bus carries it.
pub fn hid(
    kind: PartKind,
    vid: u16,
    pid: u16,
    vendor_name: &str,
    product: &str,
    serial: &str,
) -> Identity {
    Identity {
        kind,
        vendor: format!("{vid:04x}"),
        vendor_name: vendor_name.to_owned(),
        model: product.to_owned(),
        part_number: String::new(),
        serial: serial.to_owned(),
        id: format!("hid:{vid:04x}:{pid:04x}"),
        firmware: Vec::new(),
        details: Vec::new(),
    }
}

/// [`hid`] read off an enumerated device. An absent string is a descriptor
/// that carries none, kept empty rather than guessed; `resolved` is what the
/// database calls the vendor id, taken only where the descriptor names
/// nobody, a maker's own word for itself outranking a registry's.
pub fn of_hid(kind: PartKind, dev: &hidapi::DeviceInfo, resolved: &str) -> Identity {
    let announced = dev.manufacturer_string().unwrap_or_default();
    hid(
        kind,
        dev.vendor_id(),
        dev.product_id(),
        if announced.is_empty() {
            resolved
        } else {
            announced
        },
        dev.product_string().unwrap_or_default(),
        dev.serial_number().unwrap_or_default(),
    )
}

/// A part on the USB bus, from what its descriptor announces. The ids are
/// the USB-IF registry's, and the identifier names the model rather than the
/// unit: two of one part carry one id and are told apart by serial. The
/// release the descriptor states is the part's firmware.
pub fn of_usb(kind: PartKind, device: &BusDevice) -> Identity {
    Identity {
        kind,
        vendor: format!("{:04x}", device.vendor_id),
        vendor_name: device.manufacturer.clone(),
        model: device.product.clone(),
        part_number: String::new(),
        serial: device.serial.clone(),
        id: format!("usb:{:04x}:{:04x}", device.vendor_id, device.product_id),
        firmware: (!device.version.is_empty())
            .then(|| Firmware::new(FirmwareKind::Own, &device.version))
            .into_iter()
            .collect(),
        details: Vec::new(),
    }
}

/// A part on the PCI bus, under the words the database holds for its ids:
/// the bus answers ids and the kernel's classes carry no name of their own.
/// The identifier names the model rather than the unit, as [`of_usb`].
pub fn of_pci(kind: PartKind, vendor: u16, device: u16, named: &PciNames) -> Identity {
    Identity {
        kind,
        vendor: format!("{vendor:04x}"),
        vendor_name: named.vendor.clone(),
        model: named.model.clone(),
        part_number: String::new(),
        serial: String::new(),
        id: format!("pci:{vendor:04x}:{device:04x}"),
        firmware: Vec::new(),
        details: Vec::new(),
    }
}

/// A panel, from what its EDID announces. The PNP id and the product code
/// are the two things every EDID carries, so the identifier is those.
pub fn edid(
    manufacturer: &str,
    vendor_name: &str,
    product: u16,
    name: &str,
    serial: &str,
) -> Identity {
    Identity {
        kind: PartKind::Display,
        vendor: manufacturer.to_owned(),
        vendor_name: vendor_name.to_owned(),
        model: name.to_owned(),
        part_number: String::new(),
        serial: serial.to_owned(),
        id: format!("edid:{manufacturer}:{product:04x}"),
        firmware: Vec::new(),
        details: Vec::new(),
    }
}

/// A Smart Battery, from what its gauge announces over the EC. The model
/// number is the part's rather than the unit's; the serial is the unit's.
/// A zero rating is taken as one the gauge does not state, and a capacity
/// without a voltage has no energy to be spelled in.
pub fn sbs(
    manufacturer: &str,
    model: &str,
    serial: &str,
    design_capacity: u32,
    design_millivolts: u32,
    manufacture_date: Option<String>,
) -> Identity {
    let mut details = Vec::new();
    if design_millivolts != 0 {
        if design_capacity != 0 {
            details.push(Detail::DesignCapacity {
                milliamp_hours: design_capacity,
                millivolts: design_millivolts,
            });
        }
        details.push(Detail::NominalVoltage(design_millivolts));
    }
    details.extend(manufacture_date.map(Detail::ManufactureDate));
    Identity {
        kind: PartKind::Battery,
        vendor: manufacturer.to_owned(),
        vendor_name: String::new(),
        model: model.to_owned(),
        part_number: String::new(),
        serial: serial.to_owned(),
        id: format!("sbs:{model}"),
        firmware: Vec::new(),
        details,
    }
}

pub trait Part {
    fn identity(&self) -> &Identity;
}

#[cfg(test)]
mod tests {
    use super::{Detail, PartKind, of_usb, sbs};
    use crate::testing::webcam;
    use crate::usb::BusDevice;

    #[test]
    fn a_device_announcing_no_release_carries_no_firmware() {
        let silent = BusDevice {
            version: String::new(),
            ..webcam()
        };
        assert!(of_usb(PartKind::Camera, &silent).firmware.is_empty());
    }

    #[test]
    fn a_rated_and_dated_pack_carries_its_capacity_voltage_and_date() {
        let pack = sbs(
            "NVT",
            "FRANGWA",
            "0001",
            3_900,
            15_400,
            Some("2026-01-01".to_owned()),
        );
        assert_eq!(
            pack.details,
            [
                Detail::DesignCapacity {
                    milliamp_hours: 3_900,
                    millivolts: 15_400,
                },
                Detail::NominalVoltage(15_400),
                Detail::ManufactureDate("2026-01-01".to_owned()),
            ]
        );
    }

    #[test]
    fn a_capacity_without_a_voltage_is_left_out() {
        assert!(
            sbs("NVT", "FRANGWA", "0001", 3_900, 0, None)
                .details
                .is_empty()
        );
    }

    #[test]
    fn a_voltage_without_a_capacity_stands_alone() {
        assert_eq!(
            sbs("NVT", "FRANGWA", "0001", 0, 15_400, None).details,
            [Detail::NominalVoltage(15_400)]
        );
    }
}
