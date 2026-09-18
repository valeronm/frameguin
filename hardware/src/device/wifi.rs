//! The Wi-Fi module, read for the bill of materials alone.

use crate::part::{self, Detail, Identity, Part, PartKind};
use crate::udev::{self, PciNames};
use crate::wireless::{self, Radio};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Wifi {
    identity: Identity,
}

impl Part for Wifi {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Wifi {
    pub(crate) fn detect() -> Vec<Self> {
        wireless::radios()
            .iter()
            .map(|radio| Self::of(radio, &udev::pci_names(&radio.function.address)))
            .collect()
    }

    /// The running firmware is the driver's answer to an ethtool ioctl rather
    /// than a sysfs attribute, so the part carries none.
    fn of(radio: &Radio, named: &PciNames) -> Self {
        let Radio { function, mac } = radio;
        Self {
            identity: Identity {
                details: mac.clone().map(Detail::MacAddress).into_iter().collect(),
                ..part::of_pci(PartKind::Wifi, function.vendor, function.device, named)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Wifi;
    use crate::part::{Detail, Part, PartKind};
    use crate::pci::Function;
    use crate::udev::PciNames;
    use crate::wireless::Radio;

    fn discrete() -> Radio {
        Radio {
            function: Function {
                vendor: 0x14c3,
                device: 0x0717,
                address: "0000:01:00.0".to_owned(),
            },
            mac: Some("e0:c9:32:00:00:00".to_owned()),
        }
    }

    fn named() -> PciNames {
        PciNames {
            vendor: "MEDIATEK Corp.".to_owned(),
            model: "MT7925 (RZ717) Wi-Fi 7 160MHz".to_owned(),
        }
    }

    fn unnamed() -> PciNames {
        PciNames {
            vendor: String::new(),
            model: String::new(),
        }
    }

    #[test]
    fn a_radio_is_a_part_under_the_database_s_words() {
        let identity = Wifi::of(&discrete(), &named()).identity().clone();
        assert_eq!(identity.kind, PartKind::Wifi);
        assert_eq!(identity.vendor, "14c3");
        assert_eq!(identity.vendor_name, "MEDIATEK Corp.");
        assert_eq!(identity.model, "MT7925 (RZ717) Wi-Fi 7 160MHz");
        assert_eq!(identity.id, "pci:14c3:0717");
        assert_eq!(
            identity.details,
            [Detail::MacAddress("e0:c9:32:00:00:00".to_owned())]
        );
        assert!(identity.firmware.is_empty());
    }

    #[test]
    fn a_radio_the_database_does_not_name_is_still_a_part() {
        let cnvi = Radio {
            function: Function {
                vendor: 0x8086,
                device: 0xe440,
                address: "0000:00:14.3".to_owned(),
            },
            ..discrete()
        };
        let identity = Wifi::of(&cnvi, &unnamed()).identity().clone();
        assert_eq!(identity.id, "pci:8086:e440");
        assert!(identity.model.is_empty());
    }

    #[test]
    fn a_radio_with_no_address_of_its_own_carries_no_detail() {
        let silent = Radio {
            mac: None,
            ..discrete()
        };
        assert!(Wifi::of(&silent, &named()).identity().details.is_empty());
    }
}
