//! Pins the bytes, not the Rust. Every enum here is spelled once, in a serde
//! attribute, and the string it produces is what the other end matches on —
//! so a rename, a reorder or a changed `signature` is a protocol break that
//! no compiler on either side would report.

use frameguin_wire::{
    BatteryAlarm, BatteryCondition, BatteryFeature, BatteryInfo, BatteryState, Capability,
    ChargeFlow, ClickForce, Identity, PartKind, PowerLedLevel,
};
use zbus::zvariant::serialized::Context;
use zbus::zvariant::{LE, Type, to_bytes};

fn wire_string<T: serde::Serialize + Type>(value: T) -> String {
    let encoded = to_bytes(Context::new_dbus(LE, 0), &value).unwrap();
    encoded.deserialize::<String>().unwrap().0
}

#[test]
fn every_enum_crosses_the_bus_as_a_plain_string() {
    assert_eq!(Capability::SIGNATURE, "s");
    assert_eq!(BatteryFeature::SIGNATURE, "s");
    assert_eq!(PowerLedLevel::SIGNATURE, "s");
    assert_eq!(ClickForce::SIGNATURE, "s");
    assert_eq!(ChargeFlow::SIGNATURE, "s");
    assert_eq!(BatteryAlarm::SIGNATURE, "s");
}

/// The shapes the interface actually carries, as they appear in
/// introspection: `GetCapabilities` answers `as`, the power LED's
/// `GetBrightness` answers `(ys)`, `GetBatteryInfo` the battery block as a
/// struct carrying a struct.
#[test]
fn the_composite_signatures_are_the_ones_the_methods_declare() {
    assert_eq!(Vec::<Capability>::SIGNATURE, "as");
    assert_eq!(<(u8, PowerLedLevel)>::SIGNATURE, "(ys)");
    // Field order is the protocol here, the members being positional and
    // unnamed: reordering the struct silently re-maps every field a client
    // reads.
    assert_eq!(BatteryState::SIGNATURE, "(ysuu)");
    // The report carries the reading rather than restating its fields, so the
    // block above appears nested inside this one — flattening it would be a
    // protocol break that reads in the diff like a tidy-up.
    assert_eq!(BatteryInfo::SIGNATURE, "((ysuu)uuuuubbsssss)");
    // The pack's own report: cell voltages, alarms by name, and a temperature
    // in tenths of a degree.
    assert_eq!(BatteryCondition::SIGNATURE, "(auasn)");
    // A part: kind, vendor, model, serial, id, then its firmwares as
    // name/version pairs.
    assert_eq!(Identity::SIGNATURE, "(sssssa(ss))");
}

#[test]
fn part_kind_names_are_kebab_case() {
    assert_eq!(wire_string(PartKind::Mainboard), "mainboard");
    assert_eq!(wire_string(PartKind::Battery), "battery");
    assert_eq!(wire_string(PartKind::Memory), "memory");
    assert_eq!(wire_string(PartKind::Touchpad), "touchpad");
    assert_eq!(wire_string(PartKind::Touchscreen), "touchscreen");
}

#[test]
fn battery_alarm_names_are_kebab_case() {
    assert_eq!(wire_string(BatteryAlarm::OverCharged), "over-charged");
    assert_eq!(
        wire_string(BatteryAlarm::OverTemperature),
        "over-temperature"
    );
    assert_eq!(wire_string(BatteryAlarm::SafetyFault), "safety-fault");
}

#[test]
fn capability_names_are_kebab_case() {
    assert_eq!(
        wire_string(Capability::KeyboardBacklight),
        "keyboard-backlight"
    );
}

#[test]
fn battery_feature_names_are_kebab_case() {
    assert_eq!(wire_string(BatteryFeature::Condition), "condition");
    assert_eq!(wire_string(BatteryFeature::ChargeLimit), "charge-limit");
    assert_eq!(
        wire_string(BatteryFeature::ChargeCurrentLimit),
        "charge-current-limit"
    );
}

#[test]
fn power_led_level_names_are_kebab_case() {
    assert_eq!(wire_string(PowerLedLevel::Auto), "auto");
    assert_eq!(wire_string(PowerLedLevel::High), "high");
    assert_eq!(wire_string(PowerLedLevel::Medium), "medium");
    assert_eq!(wire_string(PowerLedLevel::Low), "low");
    assert_eq!(wire_string(PowerLedLevel::UltraLow), "ultra-low");
    assert_eq!(wire_string(PowerLedLevel::Off), "off");
    assert_eq!(wire_string(PowerLedLevel::Custom), "custom");
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
    for level in PowerLedLevel::ALL {
        assert_eq!(
            level != PowerLedLevel::Custom,
            level.is_settable(),
            "{level:?}"
        );
    }
}
