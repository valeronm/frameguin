//! The kernel's USB bus: the devices plugged straight into a root port, and
//! the attributes each announces.

use std::fs;
use std::path::Path;

use frameguin_wire::{Attached, DeviceError, DeviceResult, UsbSpeed};

const DEVICES: &str = "/sys/bus/usb/devices";

/// What the USB device needs of the bus.
pub trait UsbTree: Send + Sync {
    /// Every device directly on a root port, ordered by controller and port.
    fn root_devices(&self) -> DeviceResult<Vec<Attached>>;
}

pub(crate) struct Sysfs;

impl UsbTree for Sysfs {
    fn root_devices(&self) -> DeviceResult<Vec<Attached>> {
        let entries =
            fs::read_dir(DEVICES).map_err(|e| DeviceError::Failed(format!("{DEVICES}: {e}")))?;
        let mut found: Vec<Attached> = entries
            .flatten()
            .filter_map(|entry| {
                let port = root_port(entry.file_name().to_str()?)?;
                read(&entry.path(), port)
            })
            .collect();
        found.sort_by(|a, b| (&a.controller, a.root_port).cmp(&(&b.controller, b.root_port)));
        Ok(found)
    }
}

/// A device whose ids do not read is skipped: the kernel is still
/// enumerating it.
fn read(link: &Path, root_port: u8) -> Option<Attached> {
    let path = fs::canonicalize(link).ok()?;
    // `…/0000:00:14.0/usb3/3-5`: the root hub's parent is the controller.
    let controller = path.parent()?.parent()?.file_name()?.to_str()?.to_owned();
    let attribute = |name: &str| {
        fs::read_to_string(path.join(name))
            .map(|text| text.trim().to_owned())
            .unwrap_or_default()
    };
    Some(Attached {
        controller,
        root_port,
        vendor_id: hex(&attribute("idVendor"))?,
        product_id: hex(&attribute("idProduct"))?,
        product: attribute("product"),
        speed: speed(&attribute("speed")),
    })
}

/// The root port a device sits on, from its `bus-port` name; None for one
/// behind a hub (`3-5.1`), an interface (`3-5:1.0`) or a root hub (`usb3`).
fn root_port(name: &str) -> Option<u8> {
    let (bus, port) = name.split_once('-')?;
    bus.parse::<u32>().ok()?;
    port.parse().ok()
}

fn speed(text: &str) -> UsbSpeed {
    match text {
        "1.5" => UsbSpeed::Low,
        "12" => UsbSpeed::Full,
        "480" => UsbSpeed::High,
        "5000" => UsbSpeed::Super,
        "10000" => UsbSpeed::SuperPlus,
        "20000" => UsbSpeed::SuperPlus2x2,
        _ => UsbSpeed::Unknown,
    }
}

fn hex(text: &str) -> Option<u16> {
    u16::from_str_radix(text, 16).ok()
}

#[cfg(test)]
mod tests {
    use frameguin_wire::UsbSpeed;

    use super::{hex, root_port, speed};

    #[test]
    fn a_device_on_a_root_port_is_named_bus_dash_port() {
        assert_eq!(root_port("3-5"), Some(5));
        assert_eq!(root_port("2-1"), Some(1));
    }

    #[test]
    fn a_device_behind_a_hub_is_not_on_a_root_port() {
        assert_eq!(root_port("3-5.1"), None);
    }

    #[test]
    fn interfaces_and_root_hubs_are_not_devices_on_a_root_port() {
        assert_eq!(root_port("3-5:1.0"), None);
        assert_eq!(root_port("usb3"), None);
    }

    #[test]
    fn each_kernel_speed_names_its_link() {
        assert_eq!(speed("1.5"), UsbSpeed::Low);
        assert_eq!(speed("12"), UsbSpeed::Full);
        assert_eq!(speed("480"), UsbSpeed::High);
        assert_eq!(speed("5000"), UsbSpeed::Super);
        assert_eq!(speed("10000"), UsbSpeed::SuperPlus);
        assert_eq!(speed("20000"), UsbSpeed::SuperPlus2x2);
        assert_eq!(speed("unknown"), UsbSpeed::Unknown);
    }

    #[test]
    fn ids_are_read_as_hexadecimal() {
        assert_eq!(hex("32ac"), Some(0x32ac));
        assert_eq!(hex("zz"), None);
    }
}
