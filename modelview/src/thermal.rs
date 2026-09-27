//! The EC's temperature sensors and fans: the words for their reads.

use frameguin_contract::{Platform, Sensor, Temperature, ThermalLayout, ThermalState, Thresholds};
use frameguin_model::fan::FanPlacement;
use frameguin_model::sensor::SensorKind;

/// For a sensor or fan present at detection that the EC now reports absent.
pub const NOT_PRESENT: &str = "Not present";

/// The EC's own conversion constant, so a threshold set as 88 °C reads back
/// as 88.
const ZERO_CELSIUS_KELVIN: i32 = 273;

fn celsius(kelvin: u16) -> String {
    format!("{} °C", degrees(kelvin))
}

fn degrees(kelvin: u16) -> i32 {
    i32::from(kelvin) - ZERO_CELSIUS_KELVIN
}

#[must_use]
pub fn temperature_label(temperature: Temperature) -> String {
    match temperature {
        Temperature::Kelvin(kelvin) => celsius(kelvin),
        Temperature::Failed => "Read failed".to_owned(),
        Temperature::Unpowered => "Not powered".to_owned(),
        Temperature::Uncalibrated => "Not calibrated".to_owned(),
    }
}

fn sensor_kind_word(kind: SensorKind) -> &'static str {
    match kind {
        SensorKind::Processor => "Processor",
        SensorKind::NearProcessor => "Near the processor",
        SensorKind::Mainboard => "Mainboard",
        SensorKind::Memory => "Memory",
        SensorKind::Battery => "Battery",
        SensorKind::Charger => "Charger",
        SensorKind::Graphics => "Graphics",
        SensorKind::GraphicsPower => "Graphics power",
        SensorKind::PalmRest => "Palm rest",
        SensorKind::Ambient => "Ambient",
        SensorKind::PowerStage => "Power stage",
    }
}

#[must_use]
pub fn sensor_name(sensor: &Sensor) -> String {
    let Some(name) = &sensor.name else {
        return format!("Sensor {}", sensor.index);
    };
    frameguin_model::sensor::kind(name)
        .map_or_else(|| name.clone(), |kind| sensor_kind_word(kind).to_owned())
}

#[must_use]
pub fn rpm_label(rpm: u16) -> String {
    if rpm == 0 {
        "Stopped".to_owned()
    } else {
        format!("{rpm} RPM")
    }
}

/// `fans` is how many the board has, a lone unplaced fan needing no number.
#[must_use]
pub fn fan_name(platform: Platform, index: u8, fans: usize) -> String {
    match frameguin_model::fan::placement(platform, index) {
        Some(FanPlacement::Left) => "Left fan".to_owned(),
        Some(FanPlacement::Right) => "Right fan".to_owned(),
        None if fans == 1 => "Fan".to_owned(),
        None => format!("Fan {}", u16::from(index) + 1),
    }
}

/// Empty where the EC holds none.
#[must_use]
pub fn thresholds_label(thresholds: &Thresholds) -> String {
    let fan = match (thresholds.fan_off, thresholds.fan_max) {
        (Some(off), Some(max)) => Some(format!("Fan {}–{}", degrees(off), celsius(max))),
        (Some(off), None) => Some(format!("Fan from {}", celsius(off))),
        (None, Some(max)) => Some(format!("Fan full at {}", celsius(max))),
        (None, None) => None,
    };
    let trips = [
        ("Warns at", thresholds.warn),
        ("Throttles at", thresholds.high),
        ("Shuts down at", thresholds.halt),
    ]
    .into_iter()
    .filter_map(|(words, kelvin)| Some(format!("{words} {}", celsius(kelvin?))));
    fan.into_iter().chain(trips).collect::<Vec<_>>().join(" · ")
}

/// Empty where no sensor gave a temperature.
#[must_use]
pub fn thermal_summary(layout: &ThermalLayout, state: &ThermalState) -> String {
    let hottest = state
        .sensors
        .iter()
        .filter_map(|reading| match reading.temperature {
            Temperature::Kelvin(kelvin) => Some((kelvin, reading.index)),
            _ => None,
        })
        .max_by_key(|(kelvin, _)| *kelvin);
    let Some((kelvin, index)) = hottest else {
        return String::new();
    };
    match layout.sensors.iter().find(|sensor| sensor.index == index) {
        Some(sensor) => format!("{} · {}", sensor_name(sensor), celsius(kelvin)),
        None => celsius(kelvin),
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{
        Fan, Platform, Sensor, SensorReading, Temperature, ThermalLayout, ThermalState, Thresholds,
    };

    use super::{
        celsius, fan_name, rpm_label, sensor_name, temperature_label, thermal_summary,
        thresholds_label,
    };

    fn thresholds() -> Thresholds {
        Thresholds {
            index: 0,
            warn: None,
            high: Some(361),
            halt: Some(371),
            fan_off: Some(313),
            fan_max: Some(348),
        }
    }

    #[test]
    fn kelvin_converts_as_the_ec_does() {
        assert_eq!(celsius(361), "88 °C");
        assert_eq!(celsius(273), "0 °C");
        assert_eq!(celsius(250), "-23 °C");
    }

    #[test]
    fn a_sensor_that_gave_no_temperature_says_why() {
        assert_eq!(temperature_label(Temperature::Kelvin(340)), "67 °C");
        assert_eq!(temperature_label(Temperature::Failed), "Read failed");
        assert_eq!(temperature_label(Temperature::Unpowered), "Not powered");
        assert_eq!(
            temperature_label(Temperature::Uncalibrated),
            "Not calibrated"
        );
    }

    #[test]
    fn a_sensor_is_named_by_its_kind_by_the_ec_or_by_its_number() {
        let placed = Sensor {
            index: 1,
            name: Some("cpu_f75303@4d".into()),
        };
        let unmatched = Sensor {
            index: 2,
            name: Some("mystery_x@1".into()),
        };
        let unnamed = Sensor {
            index: 3,
            name: None,
        };
        assert_eq!(sensor_name(&placed), "Near the processor");
        assert_eq!(sensor_name(&unmatched), "mystery_x@1");
        assert_eq!(sensor_name(&unnamed), "Sensor 3");
    }

    #[test]
    fn a_fan_at_rest_reads_as_stopped() {
        assert_eq!(rpm_label(2400), "2400 RPM");
        assert_eq!(rpm_label(0), "Stopped");
    }

    #[test]
    fn a_fan_is_named_by_its_place_and_else_by_its_number() {
        assert_eq!(fan_name(Platform::Laptop16AmdAi300, 0, 2), "Left fan");
        assert_eq!(fan_name(Platform::Laptop16AmdAi300, 1, 2), "Right fan");
        assert_eq!(fan_name(Platform::Laptop13ProUltra3, 0, 1), "Fan");
        assert_eq!(fan_name(Platform::Laptop13ProUltra3, 1, 3), "Fan 2");
    }

    #[test]
    fn the_thresholds_read_as_the_fan_range_then_the_trip_points() {
        assert_eq!(
            thresholds_label(&thresholds()),
            "Fan 40–75 °C · Throttles at 88 °C · Shuts down at 98 °C"
        );
        let warned = Thresholds {
            warn: Some(358),
            fan_max: None,
            ..thresholds()
        };
        assert_eq!(
            thresholds_label(&warned),
            "Fan from 40 °C · Warns at 85 °C · Throttles at 88 °C · Shuts down at 98 °C"
        );
        let none = Thresholds {
            warn: None,
            high: None,
            halt: None,
            fan_off: None,
            fan_max: None,
            index: 0,
        };
        assert_eq!(thresholds_label(&none), "");
    }

    #[test]
    fn the_summary_names_the_hottest_sensor() {
        let layout = ThermalLayout {
            sensors: vec![
                Sensor {
                    index: 0,
                    name: Some("cpu_f75303@4d".into()),
                },
                Sensor {
                    index: 1,
                    name: Some("peci-temp".into()),
                },
                Sensor {
                    index: 2,
                    name: None,
                },
            ],
            fans: vec![0],
        };
        let state = ThermalState {
            sensors: vec![
                SensorReading {
                    index: 0,
                    temperature: Temperature::Kelvin(330),
                },
                SensorReading {
                    index: 1,
                    temperature: Temperature::Kelvin(340),
                },
                SensorReading {
                    index: 2,
                    temperature: Temperature::Failed,
                },
            ],
            fans: vec![Fan { index: 0, rpm: 0 }],
        };
        assert_eq!(thermal_summary(&layout, &state), "Processor · 67 °C");
        let unread = ThermalState {
            sensors: vec![SensorReading {
                index: 2,
                temperature: Temperature::Failed,
            }],
            fans: Vec::new(),
        };
        assert_eq!(thermal_summary(&layout, &unread), "");
    }
}
