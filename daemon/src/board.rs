//! What the firmware says the machine is, read from world-readable DMI sysfs.

/// Without `/dev/cros_ec`, `framework_lib` falls back to raw port I/O; on a
/// non-Framework EC every command spin-waits to a timeout, stalling the
/// first `GetCapabilities` for tens of seconds. Don't touch the EC unless the
/// firmware says this is the hardware it belongs to.
pub(crate) fn is_framework() -> bool {
    std::fs::read_to_string("/sys/class/dmi/id/sys_vendor")
        .is_ok_and(|vendor| vendor.trim() == frameguin_wire::VENDOR)
}
