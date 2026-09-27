//! What a temperature sensor sits on: the EC names one by its devicetree
//! node — a prefix for where it sits, then the chip, then `@` and its bus
//! address — and the prefix is Framework's own naming across every board's
//! devicetree.

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
    use super::{SensorKind, kind};

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
