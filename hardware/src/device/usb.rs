//! The USB devices plugged straight into the machine's root ports, read and
//! never set. Which socket a root port reaches is not this device's to say.

use std::sync::Arc;

use frameguin_wire::{Attached, DeviceResult, UsbControl};

use crate::dmi;
use crate::usb::{Sysfs, UsbTree};

pub struct Usb {
    tree: Arc<dyn UsbTree>,
}

impl Usb {
    /// Behind the vendor check, so a machine that is not a Framework one
    /// still answers with no controls.
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
    async fn attached(&self) -> DeviceResult<Vec<Attached>> {
        self.tree.root_devices()
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
}
