//! The fingerprint reader, read for the bill of materials alone. What the
//! EC's fingerprint commands drive is the power button's LED, which is
//! `power_led.rs`.

use crate::part::{self, Identity, Part, PartKind};
use crate::usb::{self, BusDevice};

/// The sensor Framework fits, by its maker's ids. Goodix sells to whoever
/// buys, so this id names the module and not the machine it is fitted to.
const GOODIX_VID: u16 = 0x27c6;
const READER_PID: u16 = 0x609c;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    identity: Identity,
}

impl Part for Fingerprint {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Fingerprint {
    pub(crate) fn detect(bus: &[BusDevice]) -> Option<Self> {
        usb::matching(bus, GOODIX_VID, &[READER_PID]).map(|device| Self {
            identity: part::of_usb(PartKind::Fingerprint, device),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Fingerprint;
    use crate::part::{Firmware, FirmwareKind, Part, PartKind};
    use crate::testing::{reader, webcam};
    use crate::usb::BusDevice;

    #[test]
    fn the_reader_is_a_part_under_its_maker_s_words() {
        let found = Fingerprint::detect(&[webcam(), reader()]).expect("the reader was found");
        let identity = found.identity();
        assert_eq!(identity.kind, PartKind::Fingerprint);
        assert_eq!(identity.vendor, "27c6");
        assert_eq!(identity.vendor_name, "Goodix Technology Co., Ltd.");
        assert_eq!(identity.model, "Goodix Fingerprint USB Device");
        assert_eq!(identity.id, "usb:27c6:609c");
        assert_eq!(
            identity.firmware,
            vec![Firmware::new(FirmwareKind::Own, "1.0.0")]
        );
    }

    #[test]
    fn a_sensor_of_another_make_is_not_this_part() {
        let other = BusDevice {
            vendor_id: 0x06cb,
            ..reader()
        };
        assert!(Fingerprint::detect(&[other]).is_none());
    }

    #[test]
    fn a_machine_with_no_reader_on_the_bus_has_none() {
        assert!(Fingerprint::detect(&[webcam()]).is_none());
    }
}
