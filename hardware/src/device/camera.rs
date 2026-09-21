//! The webcam module, read for the bill of materials alone.

use framework_lib::ccgx::hid::FRAMEWORK_VID;

use crate::part::{self, Identity, Part, PartKind};
use crate::usb::{self, BusDevice};

/// The module fitted to the Laptop 13 and 16, and the Laptop 12's own.
/// `framework_lib` names both, in a module its `rusb` feature configures
/// out — and that feature is libusb, for a read that is two sysfs files.
const WEBCAM_PIDS: [u16; 2] = [0x001c, 0x001d];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Camera {
    identity: Identity,
}

impl Part for Camera {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Camera {
    /// Framework's own ids and no other: the modules of earlier generations
    /// announce ids nothing here knows, and a camera answering for a part
    /// nobody sold reads exactly like one that is right.
    pub(crate) fn detect(bus: &[BusDevice]) -> Option<Self> {
        usb::matching(bus, FRAMEWORK_VID, &WEBCAM_PIDS).map(|device| Self {
            identity: part::of_usb(PartKind::Camera, device),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Camera;
    use crate::part::{Firmware, FirmwareKind, Part, PartKind};
    use crate::testing::{reader, webcam};
    use crate::usb::BusDevice;

    #[test]
    fn the_module_is_a_part_under_its_own_words() {
        let camera = Camera::detect(&[reader(), webcam()]).expect("the module was found");
        let identity = camera.identity();
        assert_eq!(identity.kind, PartKind::Camera);
        assert_eq!(identity.vendor, "32ac");
        assert_eq!(identity.vendor_name, "Framework");
        assert_eq!(identity.model, "Laptop Webcam Module (2nd Gen)");
        assert_eq!(identity.serial, "FRANJBCHA00000000P");
        assert_eq!(identity.id, "usb:32ac:001c");
        assert_eq!(
            identity.firmware,
            vec![Firmware::new(FirmwareKind::Own, "1.1.1")]
        );
    }

    #[test]
    fn the_laptop_12_module_is_the_same_part_under_its_own_id() {
        let twelve = BusDevice {
            product_id: 0x001d,
            product: "Laptop 12 Webcam Module".to_owned(),
            ..webcam()
        };
        let camera = Camera::detect(&[twelve]).expect("the module was found");
        assert_eq!(camera.identity().id, "usb:32ac:001d");
    }

    #[test]
    fn a_camera_of_another_make_is_not_this_part() {
        let other = BusDevice {
            vendor_id: 0x0bda,
            product_id: 0x5634,
            manufacturer: "Realtek".to_owned(),
            ..webcam()
        };
        assert!(Camera::detect(&[other]).is_none());
    }

    #[test]
    fn a_machine_with_no_module_on_the_bus_has_no_camera() {
        assert!(Camera::detect(&[reader()]).is_none());
    }
}
