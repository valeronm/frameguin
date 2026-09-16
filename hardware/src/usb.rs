//! The kernel's USB bus: the devices plugged straight into a root port, and
//! the attributes each announces.

use std::fs;
use std::path::{Path, PathBuf};

use frameguin_wire::{Attached, DeviceError, DeviceResult, NetworkLink, UsbSpeed};

use crate::{ccg3, net};

const DEVICES: &str = "/sys/bus/usb/devices";

/// A device on a root port and the sysfs directory it was read from.
pub struct RootDevice {
    pub attached: Attached,
    pub path: PathBuf,
}

/// What the USB device needs of the bus.
pub trait UsbTree: Send + Sync {
    /// Every device directly on a root port, ordered by controller and port.
    fn root_devices(&self) -> DeviceResult<Vec<RootDevice>>;
    /// The running firmware of the CCG3 card whose device directory this is,
    /// read by its firmware report alone; None where it did not answer.
    fn card_firmware(&self, device: &Path) -> Option<String>;
    /// The network interfaces of the device whose directory this is, ordered
    /// by interface name.
    fn network(&self, device: &Path) -> Vec<NetworkLink>;
}

pub(crate) struct Sysfs;

impl UsbTree for Sysfs {
    fn root_devices(&self) -> DeviceResult<Vec<RootDevice>> {
        let entries =
            fs::read_dir(DEVICES).map_err(|e| DeviceError::Failed(format!("{DEVICES}: {e}")))?;
        let mut found: Vec<RootDevice> = entries
            .flatten()
            .filter_map(|entry| {
                let port = root_port(entry.file_name().to_str()?)?;
                read(&entry.path(), port)
            })
            .collect();
        found.sort_by(|a, b| {
            (&a.attached.controller, a.attached.root_port)
                .cmp(&(&b.attached.controller, b.attached.root_port))
        });
        Ok(found)
    }

    fn card_firmware(&self, device: &Path) -> Option<String> {
        let node = vendor_hidraw(device)?;
        // hidapi's no-enumerate constructor panics once detection's own walk
        // has created a context with discovery.
        let api = hidapi::HidApi::new().ok()?;
        let path = std::ffi::CString::new(format!("/dev/{node}")).ok()?;
        let card = api.open_path(&path).ok()?;
        let mut report = [0; ccg3::REPORT_LEN];
        report[0] = ccg3::REPORT;
        card.get_feature_report(&mut report).ok()?;
        ccg3::active_version(&report)
    }

    fn network(&self, device: &Path) -> Vec<NetworkLink> {
        let mut links: Vec<NetworkLink> = interfaces(device)
            .flat_map(|interface| entries(&interface.join("net")))
            .map(|interface| net::link(&interface))
            .collect();
        links.sort_by(|a, b| a.interface.cmp(&b.interface));
        links
    }
}

/// The hidraw node of the device's interface whose report descriptor carries
/// the card's vendor usage page.
fn vendor_hidraw(device: &Path) -> Option<String> {
    interfaces(device)
        .flat_map(|interface| entries(&interface))
        .filter(|hid| {
            fs::read(hid.join("report_descriptor"))
                .is_ok_and(|descriptor| ccg3::vendor_interface(&descriptor))
        })
        .find_map(|hid| entries(&hid.join("hidraw")).next().map(|node| name(&node)))
}

/// A device whose ids do not read is skipped: the kernel is still
/// enumerating it.
fn read(link: &Path, root_port: u8) -> Option<RootDevice> {
    let path = fs::canonicalize(link).ok()?;
    // `…/0000:00:14.0/usb3/3-5`: the root hub's parent is the controller.
    let controller = path.parent()?.parent()?.file_name()?.to_str()?.to_owned();
    let attribute = |name: &str| {
        fs::read_to_string(path.join(name))
            .map(|text| text.trim().to_owned())
            .unwrap_or_default()
    };
    Some(RootDevice {
        attached: Attached {
            controller,
            root_port,
            vendor_id: hex(&attribute("idVendor"))?,
            product_id: hex(&attribute("idProduct"))?,
            manufacturer: attribute("manufacturer"),
            product: attribute("product"),
            speed: speed(&attribute("speed")),
            firmware: String::new(),
            network: Vec::new(),
        },
        path,
    })
}

/// A USB interface's directory is named with a colon, `2-2:1.0`.
fn interfaces(device: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    entries(device).filter(|interface| name(interface).contains(':'))
}

/// Empty where the directory does not read.
fn entries(dir: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The root port a device sits on, from its `bus-port` name; None for one
/// behind a hub, an interface and a root hub alike.
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
