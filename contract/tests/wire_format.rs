//! Pins the bytes, not the Rust. Every enum here is spelled once, in a serde
//! attribute, and the string it produces is what the other end matches on —
//! so a rename, a reorder or a changed `signature` is a protocol break that
//! no compiler on either side would report.

use frameguin_contract::{
    BatteryAlarm, BatteryCondition, BatteryFeature, BatteryInfo, BatteryState, Board,
    ChargeCurrentLimit, ChargeFlow, ChassisFeature, ClickForce, DeckState, ExtenderStage,
    ExtenderState, Identity, PartKind, Platform, PowerLedLevel, VENDOR,
};
use zvariant::serialized::Context;
use zvariant::{LE, Type, to_bytes};

fn wire_string<T: serde::Serialize + Type>(value: T) -> String {
    let encoded = to_bytes(Context::new_dbus(LE, 0), &value).unwrap();
    encoded.deserialize::<String>().unwrap().0
}

#[test]
fn every_enum_crosses_the_bus_as_a_plain_string() {
    assert_eq!(BatteryFeature::SIGNATURE, "s");
    assert_eq!(PowerLedLevel::SIGNATURE, "s");
    assert_eq!(ClickForce::SIGNATURE, "s");
    assert_eq!(ChargeFlow::SIGNATURE, "s");
    assert_eq!(BatteryAlarm::SIGNATURE, "s");
    assert_eq!(ExtenderStage::SIGNATURE, "s");
    assert_eq!(ChassisFeature::SIGNATURE, "s");
    assert_eq!(DeckState::SIGNATURE, "s");
}

/// The shapes the interface actually carries, as they appear in
/// introspection: the battery's `GetFeatures` answers `as`, the power LED's
/// `GetBrightness` answers `(ys)`, `GetInfo` the battery block as a struct
/// carrying a struct.
#[test]
fn the_composite_signatures_are_the_ones_the_methods_declare() {
    assert_eq!(Vec::<BatteryFeature>::SIGNATURE, "as");
    assert_eq!(<(u8, PowerLedLevel)>::SIGNATURE, "(ys)");
    // Field order is the protocol here, the members being positional and
    // unnamed: reordering the struct silently re-maps every field a client
    // reads.
    assert_eq!(BatteryState::SIGNATURE, "(ysuu)");
    // The report carries the reading rather than restating its fields, so the
    // block above appears nested inside this one — flattening it would be a
    // protocol break that reads in the diff like a tidy-up.
    assert_eq!(BatteryInfo::SIGNATURE, "((ysuu)uuuuubb)");
    // The pack's own report: cell voltages, alarms by name, and a temperature
    // in tenths of a degree.
    assert_eq!(BatteryCondition::SIGNATURE, "(auasn)");
    assert_eq!(Board::SIGNATURE, "(sss)");
    assert_eq!(ChargeCurrentLimit::SIGNATURE, "au");
    assert_eq!(ExtenderState::SIGNATURE, "(bsquq)");
    assert_eq!(Identity::SIGNATURE, "(sssssssa((sy)sss)a(sv))");
}

#[test]
fn part_kind_names_are_kebab_case() {
    assert_eq!(wire_string(PartKind::Mainboard), "mainboard");
    assert_eq!(wire_string(PartKind::Battery), "battery");
    assert_eq!(wire_string(PartKind::Memory), "memory");
    assert_eq!(wire_string(PartKind::Storage), "storage");
    assert_eq!(wire_string(PartKind::Wifi), "wifi");
    assert_eq!(wire_string(PartKind::Display), "display");
    assert_eq!(wire_string(PartKind::Touchpad), "touchpad");
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
fn battery_feature_names_are_kebab_case() {
    assert_eq!(wire_string(BatteryFeature::Condition), "condition");
    assert_eq!(wire_string(BatteryFeature::ChargeLimit), "charge-limit");
    assert_eq!(
        wire_string(BatteryFeature::ChargeCurrentLimit),
        "charge-current-limit"
    );
    assert_eq!(wire_string(BatteryFeature::Extender), "extender");
}

#[test]
fn chassis_feature_and_deck_state_names_are_kebab_case() {
    assert_eq!(wire_string(ChassisFeature::Deck), "deck");
    assert_eq!(wire_string(DeckState::Off), "off");
    assert_eq!(wire_string(DeckState::Disconnected), "disconnected");
    assert_eq!(wire_string(DeckState::TurningOn), "turning-on");
    assert_eq!(wire_string(DeckState::On), "on");
    assert_eq!(wire_string(DeckState::ForceOff), "force-off");
    assert_eq!(wire_string(DeckState::ForceOn), "force-on");
    assert_eq!(wire_string(DeckState::NoDetection), "no-detection");
}

#[test]
fn extender_stage_names_are_kebab_case() {
    assert_eq!(wire_string(ExtenderStage::Inactive), "inactive");
    assert_eq!(wire_string(ExtenderStage::First), "first");
    assert_eq!(wire_string(ExtenderStage::Second), "second");
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
fn a_power_led_level_is_named_as_it_travels() {
    for level in PowerLedLevel::ALL {
        assert_eq!(level.name(), wire_string(level));
        assert_eq!(PowerLedLevel::from_name(level.name()), Some(level));
    }
    assert_eq!(PowerLedLevel::from_name("bright"), None);
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

/// The pairing `hardware` derives is the only one a board can hold: a
/// product name another manufacturer ships cannot carry a platform with it.
#[test]
fn another_vendors_machine_takes_no_platform_whatever_it_reports() {
    let board = |vendor: &str| {
        Board::new(
            vendor.to_owned(),
            "Laptop".to_owned(),
            Platform::Laptop13Gen11,
        )
    };
    assert_eq!(board(VENDOR).platform(), Platform::Laptop13Gen11);
    assert_eq!(board("LENOVO").platform(), Platform::Unknown);
    assert_eq!(board("").platform(), Platform::Unknown);
}

#[test]
fn only_this_vendor_is_this_hardware() {
    let board = |vendor: &str| Board::new(vendor.to_owned(), String::new(), Platform::Unknown);
    assert!(board(VENDOR).is_framework());
    assert!(!board("LENOVO").is_framework());
    assert!(!board("").is_framework());
}

#[test]
fn a_platform_crosses_as_its_own_name() {
    assert_eq!(
        wire_string(Platform::Laptop13ProUltra3),
        "laptop13-pro-ultra3"
    );
    assert_eq!(wire_string(Platform::Unknown), "unknown");
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
