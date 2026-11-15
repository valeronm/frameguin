//! Intel's Management Engine, through the kernel's `mei` class.

use crate::sysfs::attribute;

const CLASS: &str = "/sys/class/mei";

/// The class also holds a graphics card's own firmware interface, under
/// another kind.
const ENGINE: &str = "mei";

/// The running Management Engine firmware's version, and None on a machine
/// with no engine.
pub(crate) fn version() -> Option<String> {
    std::fs::read_dir(CLASS).ok()?.flatten().find_map(|device| {
        let device = device.path();
        if attribute(&device, "kind")? != ENGINE {
            return None;
        }
        running(&attribute(&device, "fw_ver")?)
    })
}

/// The kernel writes a line per firmware block as `<platform>:<version>`,
/// the running firmware's first.
fn running(fw_ver: &str) -> Option<String> {
    let (_platform, version) = fw_ver.lines().next()?.split_once(':')?;
    (!version.is_empty()).then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::running;

    #[test]
    fn the_running_firmware_is_the_first_block_without_its_platform() {
        assert_eq!(
            running("0:21.0.6.1503\n0:21.0.6.1503\n0:21.0.6.1503\n").as_deref(),
            Some("21.0.6.1503")
        );
    }

    #[test]
    fn a_file_of_another_shape_gives_no_version() {
        assert_eq!(running(""), None);
        assert_eq!(running("21.0.6.1503\n"), None);
        assert_eq!(running("0:\n"), None);
    }
}
