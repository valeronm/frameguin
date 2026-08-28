//! What this board actually supports.
//!
//! One capability per exposed operation, and each probe must be a
//! side-effect-free exercise of the same code path the operation uses — never
//! a related-but-easier check (a subsystem answers a version read while the
//! command that would act on it works only on other hardware, or is not a
//! command it implements at all — see the touchscreen). Where no
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
use framework_lib::touchscreen::{HX_PID, HX_VID};

use crate::ec::Ec;
use crate::led;
use crate::touchscreen::{self, Route};

/// The controls not yet served as devices of their own; a device answers for
/// itself by being on the bus or not. `ec` is None on hardware with no
/// Framework EC, which leaves everything but the touchscreen unsupported.
/// `hid` is the one walk of the HID bus a daemon run makes, taken at startup
/// for the devices detected there and handed on here for the rest.
pub fn capabilities(ec: Option<&Ec>, hid: Option<&hidapi::HidApi>) -> Vec<wire::Capability> {
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
        if ec.power_led_level().is_ok() {
            caps.push(wire::Capability::PowerLedBrightness);
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
                caps.push(wire::Capability::PowerLedBrightnessCustom);
            }
            // Nested under the EC's own power-LED control, which this one
            // needs even though it never commands the LED through it: the
            // setter dates its write against the EC, and off is offered as
            // one level among that control's rest rather than as a control of
            // its own. The probe is the same lookup the setter makes, so the
            // two cannot come to different answers about what is reachable.
            if led::controllable_power().is_some() {
                caps.push(wire::Capability::PowerLedOff);
            }
        }
    }
    // Outside the EC's block: neither route runs through it, so a board whose
    // EC would not open can still have one of them.
    //
    // Which route this machine has is [`touchscreen::find`]'s answer rather
    // than one worked out again here, so the route qualified below is the one
    // an operation will take. What is left here is the surplus an offer needs
    // over a write, which differs by route and is why this is a match rather
    // than a condition.
    let touchscreen = match touchscreen::find(hid) {
        // The pad gates a panel rather than being one, so the board naming
        // the pad is only half of it: the controller on the bus is what says
        // anything is behind the line. Panels and mainboards are sold apart
        // and the chassis takes any pairing, so a board of the right
        // generation behind a panel with no touch would otherwise be offered
        // a switch with nothing on the end of it.
        //
        // `level` is the setter's own line request, side-effect-free and
        // failing on everything the write would fail on: a chip that will not
        // open, a locked pad, a line another driver holds.
        Some(Route::Pad(pad)) => pad.level().is_ok() && hid.is_some_and(gated_panel_present),
        // Nothing to add: the command is the controller's own, so finding the
        // controller was the whole question and no board answers for it.
        Some(Route::Panel) => true,
        None => false,
    };
    if touchscreen {
        caps.push(wire::Capability::Touchscreen);
    }
    caps
}

/// Touch controllers a board's enable pad is what gates, by the identity they
/// announce on the bus.
///
/// Keyed on the controller rather than on which panel it shipped in, as the
/// haptic touchpad is: the enable is a board signal reaching the display
/// connector, so it gates whichever touch panel is plugged into it.
///
/// A controller belongs here when the pad is how this daemon switches it, and
/// so not the Ilitek, which answers a command of its own — [`crate::panel`]
/// curates that route's controller by the same rule and for the same reason,
/// beside the code that sends it the command. What the two share is the
/// capability: they are ways into the one control, and the capability and its
/// setter name the panel rather than the route to it.
///
/// Curated, so it decides the offer and nothing else. A controller missing
/// from this list is a control not shown, never a write refused — the setter
/// never consults it.
const TOUCH_CONTROLLERS: [(u16, u16); 1] = [(HX_VID, HX_PID)];

fn gated_panel_present(hid: &hidapi::HidApi) -> bool {
    hid.device_list()
        .any(|dev| TOUCH_CONTROLLERS.contains(&(dev.vendor_id(), dev.product_id())))
}
