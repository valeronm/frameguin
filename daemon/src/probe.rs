//! What this board actually supports.
//!
//! One capability per exposed operation, and each probe must be a
//! side-effect-free exercise of the same code path the operation uses — never
//! a related-but-easier check (a board can support reading a subsystem's
//! version while its enable command works only on other hardware). Where no
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
use crate::{led, touchpad};

/// `ec` is None on hardware with no Framework EC, which leaves everything but
/// the touchpad unsupported.
pub(crate) fn capabilities(ec: Option<&Ec>) -> Vec<wire::Capability> {
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
        if ec.fp_level().is_ok() {
            caps.push(wire::Capability::FpBrightness);
            // Older EC firmware implements only command v0 of
            // FpLedLevelControl: presets high/medium/low. V1 added the
            // raw-percentage write, and the same firmware generation added
            // the ultra-low and auto levels (framework-system issue #211) —
            // so V1 support gates all of them. GET_CMD_VERSIONS is
            // side-effect-free and asks about the exact command the setters
            // use.
            if ec
                .command_supported(EcCommands::FpLedLevelControl, 1)
                .unwrap_or(false)
            {
                caps.push(wire::Capability::FpBrightnessCustom);
            }
            // Nested under the EC's own fingerprint control, which this one
            // needs even though it never commands the LED through it: the
            // setter dates its write against the EC, and off is offered as
            // one level among that control's rest rather than as a control of
            // its own. The probe is the same lookup the setter makes, so the
            // two cannot come to different answers about what is reachable.
            if led::controllable_power().is_some() {
                caps.push(wire::Capability::FpOff);
            }
        }
    }
    // One name for both haptic controls: they share the identical support
    // condition (same device, same firmware feature set).
    if touchpad::haptic_present() {
        caps.push(wire::Capability::HapticTouchpad);
    }
    caps
}
