//! The USB devices on the machine's root ports: one read, and the words a
//! reader needs for one. Which port a device sits in is `crate::port`'s.

use std::rc::Rc;

use frameguin_wire::{
    Attached, DeviceResult as Result, LinkState, NetworkLink, UsbControl, UsbSpeed,
};

use super::present;
use crate::part;

pub struct Usb<C> {
    control: Rc<C>,
}

impl<C: UsbControl> Usb<C> {
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.attached().await)?.map(|_| Self {
            control: control.clone(),
        }))
    }

    pub async fn read(&self) -> Result<Vec<Attached>> {
        self.control.attached().await
    }
}

/// What the device calls itself, and its ids where it announces no name.
#[must_use]
pub fn device_name(device: &Attached) -> String {
    let product = device.product.trim();
    if product.is_empty() {
        format!(
            "USB device {:04x}:{:04x}",
            device.vendor_id, device.product_id
        )
    } else {
        product.to_owned()
    }
}

#[must_use]
pub fn speed_label(speed: UsbSpeed) -> &'static str {
    match speed {
        UsbSpeed::Low => "1.5 Mbps",
        UsbSpeed::Full => "12 Mbps",
        UsbSpeed::High => "480 Mbps",
        UsbSpeed::Super => "5 Gbps",
        UsbSpeed::SuperPlus => "10 Gbps",
        UsbSpeed::SuperPlus2x2 => "20 Gbps",
        UsbSpeed::Unknown => "Unknown",
    }
}

#[must_use]
pub fn network_label(link: &NetworkLink) -> String {
    match (link.state, link.megabits) {
        (LinkState::Down, _) => "Switched off".to_owned(),
        (LinkState::NoCarrier, _) => "No cable".to_owned(),
        (LinkState::Up, 0) => "Connected".to_owned(),
        (LinkState::Up, m) if m >= 1000 && m % 100 == 0 => {
            format!("{} Gbps", f64::from(m) / 1000.0)
        }
        (LinkState::Up, m) => format!("{m} Mbps"),
    }
}

#[must_use]
pub fn capacity_label(bytes: u64) -> String {
    part::storage_capacity(bytes)
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{Attached, LinkState, NetworkLink, UsbSpeed};

    use super::{device_name, network_label, speed_label};

    fn link(state: LinkState, megabits: u32) -> NetworkLink {
        NetworkLink {
            interface: "enp0s13f0u2".to_owned(),
            mac: "00:e0:4c:68:02:00".to_owned(),
            state,
            megabits,
        }
    }

    #[test]
    fn a_link_that_is_up_reads_in_the_unit_its_rate_is_sold_in() {
        assert_eq!(network_label(&link(LinkState::Up, 1000)), "1 Gbps");
        assert_eq!(network_label(&link(LinkState::Up, 2500)), "2.5 Gbps");
        assert_eq!(network_label(&link(LinkState::Up, 100)), "100 Mbps");
    }

    #[test]
    fn a_link_with_no_rate_reads_as_connected() {
        assert_eq!(network_label(&link(LinkState::Up, 0)), "Connected");
    }

    #[test]
    fn a_link_that_is_not_up_says_why() {
        assert_eq!(network_label(&link(LinkState::NoCarrier, 0)), "No cable");
        assert_eq!(network_label(&link(LinkState::Down, 0)), "Switched off");
    }

    fn device(product: &str) -> Attached {
        Attached {
            controller: "0000:00:14.0".to_owned(),
            root_port: 5,
            vendor_id: 0x04c5,
            product_id: 0x2028,
            manufacturer: "iODD".to_owned(),
            product: product.to_owned(),
            speed: UsbSpeed::Super,
            firmware: String::new(),
            network: Vec::new(),
            storage: Vec::new(),
        }
    }

    #[test]
    fn a_device_is_named_by_what_it_announces() {
        assert_eq!(
            device_name(&device("HDMI Expansion Card")),
            "HDMI Expansion Card"
        );
    }

    #[test]
    fn a_device_announcing_no_name_is_named_by_its_ids() {
        assert_eq!(device_name(&device("  ")), "USB device 04c5:2028");
    }

    #[test]
    fn a_link_reads_in_the_unit_its_rate_is_sold_in() {
        assert_eq!(speed_label(UsbSpeed::Full), "12 Mbps");
        assert_eq!(speed_label(UsbSpeed::High), "480 Mbps");
        assert_eq!(speed_label(UsbSpeed::Super), "5 Gbps");
        assert_eq!(speed_label(UsbSpeed::SuperPlus2x2), "20 Gbps");
    }
}
