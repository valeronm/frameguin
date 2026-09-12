//! The drives the board carries, as their `NVMe` controllers announce them: a
//! device that is a part and no control, read for the bill of materials
//! alone.

use crate::nvme::{self, Controller};
use crate::part::{Firmware, Identity, Part, PartKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    identity: Identity,
}

impl Part for Drive {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Drive {
    /// Every drive the board carries.
    pub fn detect() -> Vec<Self> {
        nvme::controllers().iter().map(Self::of).collect()
    }

    /// The vendor is the PCI vendor id, `NVMe` naming no maker in words.
    pub fn of(controller: &Controller) -> Self {
        Self {
            identity: Identity {
                kind: PartKind::Storage,
                vendor: format!("{:04x}", controller.vendor),
                model: controller.model.clone(),
                part_number: String::new(),
                serial: controller.serial.clone(),
                size_bytes: controller.capacity,
                id: format!("pci:{:04x}:{:04x}", controller.vendor, controller.device),
                firmware: (!controller.firmware.is_empty())
                    .then(|| Firmware::new("Firmware", &controller.firmware))
                    .into_iter()
                    .collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Drive;
    use crate::nvme::Controller;
    use crate::part::{Firmware, Part, PartKind};

    fn controller(firmware: &str) -> Controller {
        Controller {
            vendor: 0x15b7,
            device: 0x5045,
            model: "SD PC SN7100S SDFPNSL-1T00".to_owned(),
            serial: "0123456789AB".to_owned(),
            firmware: firmware.to_owned(),
            capacity: 1_024_209_543_168,
        }
    }

    #[test]
    fn a_drive_is_named_by_what_its_controller_announced() {
        let identity = Drive::of(&controller("7612M000")).identity().clone();
        assert_eq!(identity.kind, PartKind::Storage);
        assert_eq!(identity.vendor, "15b7");
        assert_eq!(identity.model, "SD PC SN7100S SDFPNSL-1T00");
        assert_eq!(identity.serial, "0123456789AB");
        assert_eq!(identity.size_bytes, 1_024_209_543_168);
        assert_eq!(identity.id, "pci:15b7:5045");
        assert_eq!(identity.firmware, [Firmware::new("Firmware", "7612M000")]);
    }

    #[test]
    fn a_drive_announcing_no_revision_carries_no_firmware() {
        assert!(Drive::of(&controller("")).identity().firmware.is_empty());
    }
}
