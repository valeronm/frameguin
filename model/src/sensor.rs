//! What a temperature sensor sits on: the EC names one by its devicetree
//! node — a prefix for where it sits, then the chip, then `@` and its bus
//! address — and the prefix is Framework's own naming across every board's
//! devicetree. How high a sensor can read is here too.

use frameguin_contract::Platform;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorKind {
    /// The processor die, read over PECI or SB-TSI.
    Processor,
    /// A remote sensor beside the processor.
    NearProcessor,
    /// A sensor chip's own reading, on the mainboard.
    Mainboard,
    Memory,
    Battery,
    Charger,
    Graphics,
    /// The graphics module's voltage regulator.
    GraphicsPower,
    /// The top surface under the palm rest.
    PalmRest,
    Ambient,
    PowerStage,
}

/// The first match wins: a prefix goes before any shorter prefix it starts with.
const PREFIXES: &[(&str, SensorKind)] = &[
    ("peci", SensorKind::Processor),
    ("soc", SensorKind::Processor),
    ("amd_apu", SensorKind::Processor),
    ("cpu_", SensorKind::NearProcessor),
    ("apu_", SensorKind::NearProcessor),
    ("local_", SensorKind::Mainboard),
    ("ddr_", SensorKind::Memory),
    ("memory_", SensorKind::Memory),
    ("battery", SensorKind::Battery),
    ("charger", SensorKind::Charger),
    ("gpu_vr", SensorKind::GraphicsPower),
    ("gpu", SensorKind::Graphics),
    ("top_skin", SensorKind::PalmRest),
    ("ambient", SensorKind::Ambient),
    ("power_", SensorKind::PowerStage),
];

/// The EC's own conversion constant, so a threshold set as 88 °C reads back
/// as 88.
pub const ZERO_CELSIUS_KELVIN: u16 = 273;

/// The highest a board's processor die sensor can read, in kelvin, where it
/// is read over PECI: the EC works that temperature out from its margin
/// below a junction maximum its firmware is built with,
/// `CONFIG_PLATFORM_EC_PECI_TJMAX` or the `CONFIG_PECI_TJMAX` older branches
/// define.
#[must_use]
pub const fn ceiling(platform: Platform, kind: SensorKind) -> Option<u16> {
    let junction_maximum = match (platform, kind) {
        (Platform::Laptop13ProUltra3, SensorKind::Processor) => 100,
        (
            Platform::Laptop13Ultra1 | Platform::Laptop12Gen13 | Platform::Laptop12Core3,
            SensorKind::Processor,
        ) => 110,
        _ => return None,
    };
    Some(junction_maximum + ZERO_CELSIUS_KELVIN)
}

/// None for a name no rule knows.
#[must_use]
pub fn kind(name: &str) -> Option<SensorKind> {
    let node = name.split_once('@').map_or(name, |(node, _)| node);
    let node = node.to_lowercase().replace('-', "_");
    PREFIXES
        .iter()
        .find(|(prefix, _)| node.starts_with(prefix))
        .map(|(_, kind)| *kind)
}

#[cfg(test)]
mod tests {
    use frameguin_contract::Platform;

    use super::{SensorKind, ceiling, kind};

    #[test]
    fn the_processor_die_reads_no_higher_than_its_firmwares_junction_maximum() {
        assert_eq!(
            ceiling(Platform::Laptop13ProUltra3, SensorKind::Processor),
            Some(373)
        );
        assert_eq!(
            ceiling(Platform::Laptop13Ultra1, SensorKind::Processor),
            Some(383)
        );
        assert_eq!(
            ceiling(Platform::Laptop12Gen13, SensorKind::Processor),
            Some(383)
        );
        assert_eq!(
            ceiling(Platform::Laptop12Core3, SensorKind::Processor),
            Some(383)
        );
    }

    #[test]
    fn a_thermistor_has_no_ceiling() {
        assert_eq!(
            ceiling(Platform::Laptop13ProUltra3, SensorKind::NearProcessor),
            None
        );
    }

    #[test]
    fn a_board_whose_firmware_sets_none_has_no_ceiling() {
        assert_eq!(
            ceiling(Platform::Laptop13AmdAi300, SensorKind::Processor),
            None
        );
        assert_eq!(ceiling(Platform::Unknown, SensorKind::Processor), None);
    }

    #[test]
    fn the_laptop_13_pros_runtime_names_place_their_sensors() {
        assert_eq!(kind("local_f75397@4c"), Some(SensorKind::Mainboard));
        assert_eq!(kind("cpu_f75303@4d"), Some(SensorKind::NearProcessor));
        assert_eq!(kind("battery_temp@b"), Some(SensorKind::Battery));
        assert_eq!(kind("ddr_f75303@4d"), Some(SensorKind::Memory));
        assert_eq!(kind("peci-temp"), Some(SensorKind::Processor));
    }

    #[test]
    fn devicetree_names_place_their_sensors_across_boards() {
        assert_eq!(kind("gpu-vr-f75303"), Some(SensorKind::GraphicsPower));
        assert_eq!(kind("gpu-amdr23m"), Some(SensorKind::Graphics));
        assert_eq!(kind("top-skin-f75303"), Some(SensorKind::PalmRest));
        assert_eq!(kind("amd-apu"), Some(SensorKind::Processor));
        assert_eq!(kind("apu-f75303"), Some(SensorKind::NearProcessor));
        assert_eq!(kind("soc-temp"), Some(SensorKind::Processor));
        assert_eq!(kind("charger-temp"), Some(SensorKind::Charger));
        assert_eq!(kind("memory-f75303"), Some(SensorKind::Memory));
        assert_eq!(kind("ambient-f75303"), Some(SensorKind::Ambient));
        assert_eq!(kind("power-f75303"), Some(SensorKind::PowerStage));
    }

    #[test]
    fn a_name_no_rule_matches_has_no_kind() {
        assert_eq!(kind("mystery_x@1"), None);
    }
}
