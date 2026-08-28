//! What this board actually supports.
//!
//! One capability per exposed operation, and each probe must be a
//! side-effect-free exercise of the same code path the operation uses — never
//! a related-but-easier check (a subsystem answers a version read while the
//! command that would act on it works only on other hardware, or is not a
//! command it implements at all — see [`crate::panel::controller`]). Where no
//! harmless same-path probe exists, hardcode the support condition here
//! instead of probing something adjacent. The get-side probes below stand in
//! for their setters only because those EC command pairs ship together in
//! every firmware.
//!
//! A probe decides what to offer, never what to accept: it runs once per
//! daemon lifetime, so one transient read denying a capability would deny it
//! for the whole run. Setters validate against the thing itself.

use frameguin_wire as wire;
use framework_lib::chromium_ec::command::EcCommands;

use crate::ec::Ec;

/// The controls not yet served as devices of their own; a device answers for
/// itself by being on the bus or not. `ec` is None on hardware with no
/// Framework EC, which leaves every one of them unsupported.
pub fn capabilities(ec: Option<&Ec>) -> Vec<wire::Capability> {
    let mut caps = Vec::new();
    if let Some(ec) = ec {
        // The report's own walk, run for the one thing that can stop it
        // answering rather than for a version or a neighbouring command: a
        // pack that reports nothing here is exactly the one whose state cannot
        // be shown.
        let battery = ec.battery_present();
        if battery {
            caps.push(wire::Capability::Battery);
            // The getter's own read, and the only probe here that
            // reaches the pack rather than the EC: what it answers for is the
            // I2C passthrough working, which nothing about a readable memmap
            // promises. Nested under a readable pack because these are lines
            // of its report rather than readings offered on their own — and
            // because a mainboard running with no battery must not spend
            // transfers asking one what it thinks.
            if ec.battery_condition().is_some() {
                caps.push(wire::Capability::BatteryCondition);
            }
        }
        if ec.charge_limit().is_ok() {
            caps.push(wire::Capability::ChargeLimit);
        }
        // No same-path probe exists: the charge current limit is write-only,
        // with no readback in any command version (framework-system issue
        // #180). GET_CMD_VERSIONS is the closest harmless stand-in — it is
        // side-effect-free and asks about the very command the setter sends.
        // The battery joins it because a limit is only ever expressed as a
        // share of what the pack asks for: without its capacity the control
        // has no rate to offer, so claiming it would offer a dead one. The
        // reading above answers that — a pack's capacity and its state come
        // from the same memmap block, so a readable one has both.
        // An EC that won't answer is read as "no": a probe runs once per
        // daemon lifetime, so offering on a silent read would keep offering a
        // control that may not be there for the whole run.
        if ec
            .command_supported(EcCommands::ChargeCurrentLimit, 0)
            .unwrap_or(false)
            && battery
        {
            caps.push(wire::Capability::ChargeCurrentLimit);
        }
        if ec.keyboard_backlight().is_ok() {
            caps.push(wire::Capability::KeyboardBacklight);
        }
    }
    caps
}
