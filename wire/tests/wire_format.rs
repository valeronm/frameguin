//! Pins the bytes, not the Rust. Every enum here is spelled once, in a serde
//! attribute, and the string it produces is what the other end matches on —
//! so a rename, a reorder or a changed `signature` is a protocol break that
//! no compiler on either side would report.

use frameguin_wire::{BatteryState, Capability, ChargeFlow, ClickForce, FpLevel};
use zbus::zvariant::serialized::Context;
use zbus::zvariant::{to_bytes, Type, LE};

fn wire_string<T: serde::Serialize + Type>(value: T) -> String {
    let encoded = to_bytes(Context::new_dbus(LE, 0), &value).unwrap();
    encoded.deserialize::<String>().unwrap().0
}

#[test]
fn every_enum_crosses_the_bus_as_a_plain_string() {
    assert_eq!(Capability::SIGNATURE, "s");
    assert_eq!(FpLevel::SIGNATURE, "s");
    assert_eq!(ClickForce::SIGNATURE, "s");
    assert_eq!(ChargeFlow::SIGNATURE, "s");
}

/// The shapes the interface actually carries, as they appear in
/// introspection: `GetCapabilities` answers `as`, `GetFingerprintBrightness`
/// answers `(ys)`, `GetBatteryState` the battery block as a struct.
#[test]
fn the_composite_signatures_are_the_ones_the_methods_declare() {
    assert_eq!(Vec::<Capability>::SIGNATURE, "as");
    assert_eq!(<(u8, FpLevel)>::SIGNATURE, "(ys)");
    // Field order is the protocol here, the members being positional and
    // unnamed: reordering the struct silently re-maps every field a client
    // reads.
    assert_eq!(BatteryState::SIGNATURE, "(ysuu)");
}

#[test]
fn capability_names_are_kebab_case() {
    assert_eq!(wire_string(Capability::BatteryState), "battery-state");
    assert_eq!(wire_string(Capability::ChargeLimit), "charge-limit");
    assert_eq!(
        wire_string(Capability::ChargeCurrentLimit),
        "charge-current-limit"
    );
    assert_eq!(
        wire_string(Capability::KeyboardBacklight),
        "keyboard-backlight"
    );
    assert_eq!(wire_string(Capability::FpBrightness), "fp-brightness");
    assert_eq!(
        wire_string(Capability::FpBrightnessCustom),
        "fp-brightness-custom"
    );
    assert_eq!(wire_string(Capability::FpOff), "fp-off");
    assert_eq!(wire_string(Capability::HapticTouchpad), "haptic-touchpad");
}

#[test]
fn fingerprint_level_names_are_kebab_case() {
    assert_eq!(wire_string(FpLevel::Auto), "auto");
    assert_eq!(wire_string(FpLevel::High), "high");
    assert_eq!(wire_string(FpLevel::Medium), "medium");
    assert_eq!(wire_string(FpLevel::Low), "low");
    assert_eq!(wire_string(FpLevel::UltraLow), "ultra-low");
    assert_eq!(wire_string(FpLevel::Off), "off");
    assert_eq!(wire_string(FpLevel::Custom), "custom");
}

#[test]
fn click_force_names_are_kebab_case() {
    assert_eq!(wire_string(ClickForce::Low), "low");
    assert_eq!(wire_string(ClickForce::Medium), "medium");
    assert_eq!(wire_string(ClickForce::High), "high");
}

#[test]
fn charge_flow_names_are_kebab_case() {
    assert_eq!(wire_string(ChargeFlow::Charging), "charging");
    assert_eq!(wire_string(ChargeFlow::Discharging), "discharging");
    assert_eq!(wire_string(ChargeFlow::Idle), "idle");
}

/// Custom is the one level the EC reports but will not take.
#[test]
fn every_level_but_custom_is_settable() {
    for level in FpLevel::ALL {
        assert_eq!(level != FpLevel::Custom, level.is_settable(), "{level:?}");
    }
}
