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

use std::sync::Mutex;

use frameguin_wire as wire;
use framework_lib::chromium_ec::CrosEc;
use framework_lib::chromium_ec::command::EcCommands;

use crate::ec::{battery_state, kbd_backlight_percent};
use crate::{led, touchpad};

/// `ec` is None on hardware with no Framework EC, which leaves everything but
/// the touchpad unsupported. Takes the mutex rather than a guard so the EC is
/// released before the touchpad's HID enumeration, which needs no EC.
pub(crate) fn capabilities(ec: Option<&Mutex<CrosEc>>) -> Vec<wire::Capability> {
    let mut caps = Vec::new();
    if let Some(ec) = ec {
        let ec = &*ec.lock().unwrap();
        // The getter's own read, run for its answer rather than for a version
        // or a neighbouring command: a pack that reports nothing here is
        // exactly the one whose state cannot be shown.
        let battery = battery_state(ec).is_some();
        if battery {
            caps.push(wire::Capability::BatteryState);
        }
        if ec.get_charge_limit().is_ok() {
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
        if ec
            .cmd_version_supported(EcCommands::ChargeCurrentLimit as u32, 0)
            .unwrap_or(false)
            && battery
        {
            caps.push(wire::Capability::ChargeCurrentLimit);
        }
        if kbd_backlight_percent(ec).is_ok() {
            caps.push(wire::Capability::KeyboardBacklight);
        }
        if ec.get_fp_led_level().is_ok() {
            caps.push(wire::Capability::FpBrightness);
            // Older EC firmware implements only command v0 of
            // FpLedLevelControl: presets high/medium/low. V1 added the
            // raw-percentage write, and the same firmware generation added
            // the ultra-low and auto levels (framework-system issue #211) —
            // so V1 support gates all of them. GET_CMD_VERSIONS is
            // side-effect-free and asks about the exact command the setters
            // use.
            if ec
                .cmd_version_supported(EcCommands::FpLedLevelControl as u32, 1)
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
