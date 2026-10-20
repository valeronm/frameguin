//! The USB devices plugged straight into the machine's root ports, read and
//! never set. Which socket a root port reaches is not this device's to say.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use frameguin_contract::{Attached, Board, DeviceResult, UsbControl};
use framework_lib::ccgx::hid::{ALL_CARD_PIDS, FRAMEWORK_VID};

use crate::usb::{Sysfs, UsbTree};

pub struct Usb {
    tree: Arc<dyn UsbTree>,
    /// A card's firmware report is a USB round trip of tens of milliseconds.
    /// A swap or an update reconnects the card under a new `devnum`.
    firmware: Mutex<HashMap<(PathBuf, u16), String>>,
}

impl Usb {
    /// A machine that is not a Framework one answers with no controls at all.
    pub(crate) fn detect(board: &Board) -> Option<Self> {
        if !board.is_framework() {
            return None;
        }
        Self::new(Arc::new(Sysfs))
    }

    /// None where the bus cannot be listed.
    pub fn new(tree: Arc<dyn UsbTree>) -> Option<Self> {
        tree.root_devices().is_ok().then(|| Self {
            tree,
            firmware: Mutex::default(),
        })
    }
}

impl UsbControl for Usb {
    async fn attached(&self) -> DeviceResult<Vec<Attached>> {
        let found = self.tree.root_devices()?;
        let mut known = self.firmware.lock().unwrap_or_else(PoisonError::into_inner);
        // A card gone or reconnected since the last walk leaves no entry.
        let mut previous = std::mem::take(&mut *known);
        Ok(found
            .into_iter()
            .map(|found| {
                let mut attached = found.attached;
                attached.network = self.tree.network(&found.path);
                attached.storage = self.tree.storage(&found.path);
                if attached.vendor_id == FRAMEWORK_VID
                    && ALL_CARD_PIDS.contains(&attached.product_id)
                {
                    let key = (found.path, found.connection);
                    if let Some(version) = previous
                        .remove(&key)
                        .or_else(|| self.tree.card_firmware(&key.0))
                    {
                        attached.firmware.clone_from(&version);
                        known.insert(key, version);
                    }
                }
                attached
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_contract::UsbControl;

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
        assert_eq!(attached[2].firmware, "3.0.10.06A");
        assert_eq!(attached[0].firmware, "");
        assert_eq!(attached[1].firmware, "");
    }

    #[test]
    fn a_card_is_asked_for_its_firmware_once_per_connection() {
        let hub = Arc::new(Hub::default());
        let usb = Usb::new(hub.clone()).expect("the bus listed");
        ready(usb.attached()).unwrap();
        let attached = ready(usb.attached()).unwrap();
        assert_eq!(hub.asked.lock().unwrap().len(), 1);
        assert_eq!(attached[2].firmware, "3.0.10.06A");
        *hub.connection.lock().unwrap() += 1;
        ready(usb.attached()).unwrap();
        assert_eq!(hub.asked.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_card_whose_report_did_not_answer_is_asked_again() {
        let hub = Arc::new(Hub {
            firmware: None,
            ..Hub::default()
        });
        let usb = Usb::new(hub.clone()).expect("the bus listed");
        ready(usb.attached()).unwrap();
        ready(usb.attached()).unwrap();
        assert_eq!(hub.asked.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_card_whose_report_did_not_answer_is_still_attached() {
        let hub = Hub {
            firmware: None,
            ..Hub::default()
        };
        let usb = Usb::new(Arc::new(hub)).expect("the bus listed");
        let attached = ready(usb.attached()).unwrap();
        assert_eq!(attached.len(), 3);
        assert_eq!(attached[2].firmware, "");
    }
}
