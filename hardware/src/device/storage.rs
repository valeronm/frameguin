//! The drives the board carries: a device that is a part and no control,
//! read for the bill of materials alone.

use crate::nvme::{self, Controller};
use crate::part::{self, Detail, Firmware, FirmwareKind, Identity, Part, PartKind};
use crate::udev::{self, PciNames};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Drive {
    identity: Identity,
}

impl Part for Drive {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Drive {
    pub(crate) fn detect() -> Vec<Self> {
        nvme::controllers()
            .iter()
            .map(|controller| Self::of(controller, &udev::pci_names(&controller.address)))
            .collect()
    }

    /// The vendor is the PCI vendor id, `NVMe` naming no maker in words. The
    /// database's model outranks the drive's own, which becomes the part
    /// number: a drive labels itself with what it is ordered by rather than
    /// with what it is sold as.
    fn of(controller: &Controller, named: &PciNames) -> Self {
        let (model, part_number) = if named.model.is_empty() {
            (controller.model.clone(), String::new())
        } else {
            (named.model.clone(), controller.model.clone())
        };
        Self {
            identity: Identity {
                model,
                part_number,
                serial: controller.serial.clone(),
                firmware: (!controller.firmware.is_empty())
                    .then(|| Firmware::new(FirmwareKind::Drive, &controller.firmware))
                    .into_iter()
                    .collect(),
                details: controller
                    .capacity
                    .map(Detail::StorageCapacity)
                    .into_iter()
                    .collect(),
                ..part::of_pci(
                    PartKind::Storage,
                    controller.vendor,
                    controller.device,
                    named,
                )
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Drive;
    use crate::nvme::Controller;
    use crate::part::{Detail, Firmware, FirmwareKind, Part, PartKind};
    use crate::udev::PciNames;

    fn named() -> PciNames {
        PciNames {
            vendor: "Sandisk Corp".to_owned(),
            model: "WD_BLACK SN7100".to_owned(),
        }
    }

    fn unnamed() -> PciNames {
        PciNames {
            vendor: String::new(),
            model: String::new(),
        }
    }

    fn controller(firmware: &str) -> Controller {
        Controller {
            vendor: 0x15b7,
            device: 0x5045,
            address: "0000:01:00.0".to_owned(),
            model: "SD PC SN7100S SDFPNSL-1T00".to_owned(),
            serial: "0123456789AB".to_owned(),
            firmware: firmware.to_owned(),
            capacity: Some(1_024_209_543_168),
        }
    }

    #[test]
    fn a_drive_is_named_by_the_database_and_numbered_by_its_own_label() {
        let identity = Drive::of(&controller("7612M000"), &named())
            .identity()
            .clone();
        assert_eq!(identity.kind, PartKind::Storage);
        assert_eq!(identity.vendor, "15b7");
        assert_eq!(identity.vendor_name, "Sandisk Corp");
        assert_eq!(identity.model, "WD_BLACK SN7100");
        assert_eq!(identity.part_number, "SD PC SN7100S SDFPNSL-1T00");
        assert_eq!(identity.serial, "0123456789AB");
        assert_eq!(
            identity.details,
            [Detail::StorageCapacity(1_024_209_543_168)]
        );
        assert_eq!(identity.id, "pci:15b7:5045");
        assert_eq!(
            identity.firmware,
            [Firmware::new(FirmwareKind::Drive, "7612M000")]
        );
    }

    #[test]
    fn a_drive_the_database_does_not_name_keeps_its_label_as_the_model() {
        let identity = Drive::of(&controller("7612M000"), &unnamed())
            .identity()
            .clone();
        assert_eq!(identity.model, "SD PC SN7100S SDFPNSL-1T00");
        assert!(identity.part_number.is_empty());
        assert!(identity.vendor_name.is_empty());
    }

    #[test]
    fn a_drive_announcing_no_revision_carries_no_firmware() {
        assert!(
            Drive::of(&controller(""), &unnamed())
                .identity()
                .firmware
                .is_empty()
        );
    }
}
