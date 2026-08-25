//! What the firmware says the machine is, read from world-readable DMI sysfs.

/// One DMI field, trimmed of the newline sysfs ends every one of them with.
fn dmi(field: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/dmi/id/{field}"))
        .ok()
        .map(|value| value.trim().to_owned())
}

/// Without `/dev/cros_ec`, `framework_lib` falls back to raw port I/O; on a
/// non-Framework EC every command spin-waits to a timeout, stalling the
/// first `GetCapabilities` for tens of seconds. Don't touch the EC unless the
/// firmware says this is the hardware it belongs to.
pub(crate) fn is_framework() -> bool {
    dmi("sys_vendor").as_deref() == Some(frameguin_wire::VENDOR)
}

/// The mainboard, under the name its firmware gives it — and the mainboard
/// only, which is what makes this the right question to ask about a
/// processor pad and the wrong one to ask about anything plugged into it.
///
/// Read here rather than taken from `framework_lib`, whose `get_platform`
/// answers with a type its crate keeps private and so unnameable from
/// outside. The string this matches is the one that library maps too.
pub(crate) fn product() -> Option<String> {
    dmi("product_name")
}
