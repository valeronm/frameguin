//! The kernel's USB bus: the devices plugged straight into a root port, and
//! the attributes each announces.

use std::fs;
use std::path::{Path, PathBuf};

use frameguin_contract::{Attached, DeviceError, DeviceResult, NetworkLink, UsbSpeed};

use crate::{ccg3, net, scsi};

const DEVICES: &str = "/sys/bus/usb/devices";

/// A device on a root port and the sysfs directory it was read from.
pub struct RootDevice {
    pub attached: Attached,
    pub path: PathBuf,
}

/// What a device on the bus announces about itself, each string empty where
/// its descriptor carries none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: String,
    pub product: String,
    pub serial: String,
    /// `bcdDevice`, the release the device states for itself.
    pub version: String,
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
    /// The capacity of each disk of the device whose directory this is.
    fn storage(&self, device: &Path) -> Vec<u64>;
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
        let mut links: Vec<NetworkLink> = addressed(device)
            .flat_map(|interface| entries(&interface.join("net")))
            .map(|interface| net::link(&interface))
            .collect();
        links.sort_by(|a, b| a.interface.cmp(&b.interface));
        links
    }

    fn storage(&self, device: &Path) -> Vec<u64> {
        addressed(device)
            .flat_map(|interface| prefixed(&interface, "host"))
            .flat_map(|host| prefixed(&host, "target"))
            .flat_map(|target| addressed(&target))
            .filter_map(|disk| scsi::capacity(&disk))
            .collect()
    }
}

/// The hidraw node of the device's interface whose report descriptor carries
/// the card's vendor usage page.
fn vendor_hidraw(device: &Path) -> Option<String> {
    addressed(device)
        .flat_map(|interface| entries(&interface))
        .filter(|hid| {
            fs::read(hid.join("report_descriptor"))
                .is_ok_and(|descriptor| ccg3::vendor_interface(&descriptor))
        })
        .find_map(|hid| entries(&hid.join("hidraw")).next().map(|node| name(&node)))
}

/// Every device on the bus, hubs and the devices behind them included. One
/// walk answers for every part read off the bus, a walk being a directory
/// read and six attributes for each device on it.
pub(crate) fn devices() -> Vec<BusDevice> {
    entries(Path::new(DEVICES))
        .filter_map(|path| announced(&path))
        .collect()
}

/// The device announcing one of `products` for `vendor`, and None where the
/// bus carries no such device.
pub(crate) fn matching<'a>(
    devices: &'a [BusDevice],
    vendor: u16,
    products: &[u16],
) -> Option<&'a BusDevice> {
    devices
        .iter()
        .find(|device| device.vendor_id == vendor && products.contains(&device.product_id))
}

/// A device whose ids do not read is skipped: the kernel is still
/// enumerating it.
fn read(link: &Path, root_port: u8) -> Option<RootDevice> {
    let path = fs::canonicalize(link).ok()?;
    // `…/0000:00:14.0/usb3/3-5`: the root hub's parent is the controller.
    let controller = path.parent()?.parent()?.file_name()?.to_str()?.to_owned();
    let attribute = |name: &str| attribute(&path, name);
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
            storage: Vec::new(),
        },
        path,
    })
}

/// None for a directory that states no vendor, which is every interface and
/// a device the kernel is still enumerating.
fn announced(path: &Path) -> Option<BusDevice> {
    let attribute = |name: &str| attribute(path, name);
    Some(BusDevice {
        vendor_id: hex(&attribute("idVendor"))?,
        product_id: hex(&attribute("idProduct"))?,
        manufacturer: attribute("manufacturer"),
        product: attribute("product"),
        serial: attribute("serial"),
        version: release(&attribute("bcdDevice")),
    })
}

/// The release as its vendor spells it: the major byte, then the minor
/// byte's two digits apart — `1.1.1` for `0111`. Empty where the descriptor
/// holds something other than the four decimal digits the coding allows.
fn release(bcd: &str) -> String {
    let coded: u16 = match bcd.parse() {
        Ok(coded) if bcd.len() == 4 && bcd.bytes().all(|digit| digit.is_ascii_digit()) => coded,
        _ => return String::new(),
    };
    format!("{}.{}.{}", coded / 100, coded / 10 % 10, coded % 10)
}

/// A USB interface's directory and a SCSI device's are named by an address
/// with a colon, `2-2:1.0` and `1:0:0:2`.
fn addressed(dir: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    entries(dir).filter(|child| name(child).contains(':'))
}

fn prefixed(dir: &Path, prefix: &'static str) -> impl Iterator<Item = PathBuf> + use<> {
    entries(dir).filter(move |entry| name(entry).starts_with(prefix))
}

/// Empty where the attribute does not read, which is a device that carries
/// none of that name.
fn attribute(path: &Path, name: &str) -> String {
    fs::read_to_string(path.join(name))
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
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
    use frameguin_contract::UsbSpeed;

    use super::{hex, release, root_port, speed};

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
    fn a_release_is_a_major_byte_and_two_digits() {
        assert_eq!(release("0111"), "1.1.1");
        assert_eq!(release("0100"), "1.0.0");
        assert_eq!(release("0011"), "0.1.1");
        assert_eq!(release("1234"), "12.3.4");
    }

    #[test]
    fn a_release_outside_the_coding_is_no_release() {
        assert_eq!(release(""), "");
        assert_eq!(release("111"), "");
        assert_eq!(release("01a1"), "");
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
