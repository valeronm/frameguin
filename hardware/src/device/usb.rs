//! The USB devices plugged straight into the machine's root ports, read and
//! never set. Which socket a root port reaches is not this device's to say.

use std::sync::Arc;

use frameguin_wire::{Attached, DeviceResult, UsbControl};
use framework_lib::ccgx::hid::{ALL_CARD_PIDS, FRAMEWORK_VID};

use crate::dmi;
use crate::usb::{Sysfs, UsbTree};

pub struct Usb {
    tree: Arc<dyn UsbTree>,
}

impl Usb {
    /// A machine that is not a Framework one answers with no controls at all.
    pub(crate) fn detect() -> Option<Self> {
        dmi::is_framework().then_some(())?;
        Self::new(Arc::new(Sysfs))
    }

    /// None where the bus cannot be listed.
    pub fn new(tree: Arc<dyn UsbTree>) -> Option<Self> {
        tree.root_devices().is_ok().then_some(Self { tree })
    }
}

impl UsbControl for Usb {
    /// A card swapped or reflashed between two readings must not be able to
    /// report its predecessor's version.
    async fn attached(&self) -> DeviceResult<Vec<Attached>> {
        Ok(self
            .tree
            .root_devices()?
            .into_iter()
            .map(|found| {
                let mut attached = found.attached;
                if attached.vendor_id == FRAMEWORK_VID
                    && ALL_CARD_PIDS.contains(&attached.product_id)
                {
                    attached.firmware = self.tree.card_firmware(&found.path).unwrap_or_default();
                }
                attached
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_wire::UsbControl;

    use super::Usb;
    use crate::testing::{Hub, ready};

    #[test]
    fn every_device_on_a_root_port_is_attached() {
        let usb = Usb::new(Arc::new(Hub::default())).expect("the bus listed");
        assert_eq!(ready(usb.attached()).unwrap(), Hub::default().devices);
    }

    #[test]
    fn a_machine_whose_usb_bus_cannot_be_listed_has_no_device() {
        let hub = Hub {
            refusing: true,
            ..Hub::default()
        };
        assert!(Usb::new(Arc::new(hub)).is_none());
    }

    #[test]
    fn firmware_is_asked_of_framework_display_cards_alone() {
        let hub = Arc::new(Hub::default());
        let usb = Usb::new(hub.clone()).expect("the bus listed");
        let attached = ready(usb.attached()).unwrap();
        assert_eq!(hub.asked.lock().unwrap().len(), 1);
        assert_eq!(attached[1].firmware, "3.0.10.06A");
        assert_eq!(attached[0].firmware, "");
    }

    #[test]
    fn a_card_whose_report_did_not_answer_is_still_attached() {
        let hub = Hub {
            firmware: None,
            ..Hub::default()
        };
        let usb = Usb::new(Arc::new(hub)).expect("the bus listed");
        let attached = ready(usb.attached()).unwrap();
        assert_eq!(attached.len(), 2);
        assert_eq!(attached[1].firmware, "");
    }
}
