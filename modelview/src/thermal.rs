//! The EC's temperature sensors and fans: the words for their reads, and
//! where a sensor's thresholds sit along its track.

use frameguin_contract::{
    Fan, FanDuty, Platform, Sensor, Temperature, ThermalLayout, ThermalState, Thresholds,
};
use frameguin_model::fan::FanPlacement;
use frameguin_model::sensor::{SensorKind, ZERO_CELSIUS_KELVIN};

use crate::battery::reading::percent_label;

/// For a sensor or fan present at detection that the EC now reports absent.
pub const NOT_PRESENT: &str = "Not present";

fn celsius(kelvin: u16) -> String {
    format!("{} °C", degrees(kelvin))
}

fn degrees(kelvin: u16) -> i32 {
    i32::from(kelvin) - i32::from(ZERO_CELSIUS_KELVIN)
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
        SensorKind::Processor => "Processor die",
        SensorKind::NearProcessor => "Processor",
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

/// A fan's speed, with how hard the EC is driving it where `duties` holds
/// the fan's own.
#[must_use]
pub fn fan_label(fan: &Fan, duties: &[FanDuty]) -> String {
    let speed = rpm_label(fan.rpm);
    match duties.iter().find(|duty| duty.index == fan.index) {
        Some(duty) if fan.rpm > 0 || duty.percent > 0 => {
            format!("{speed} · {}", percent_label(duty.percent))
        }
        _ => speed,
    }
}

fn rpm_label(rpm: u16) -> String {
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
fn thresholds_label(thresholds: &Thresholds) -> String {
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

pub const FAN_RANGE: &str = "Fan range";
pub const THROTTLE: &str = "Throttle";
pub const SHUTDOWN: &str = "Shutdown";

/// One sensor's thresholds laid along a line, every position in thousandths
/// of its length.
#[derive(PartialEq, Eq, Debug)]
pub struct Track {
    pub ramp: Option<(u16, u16)>,
    /// Where the EC asks for throttling or throttles.
    pub cautions: Vec<u16>,
    pub shutdown: Option<u16>,
    pub dot: Option<u16>,
    /// Most severe first and the scale's own ends last, the order a label
    /// with no room gives way in.
    pub ticks: Vec<(u16, String)>,
    /// The thresholds drawn, in words.
    pub description: String,
}

const TRACK_START_KELVIN: u16 = ZERO_CELSIUS_KELVIN + 20;
const TRACK_END_KELVIN: u16 = ZERO_CELSIUS_KELVIN + 100;

fn ceiling(platform: Platform, sensor: &Sensor) -> Option<u16> {
    let kind = frameguin_model::sensor::kind(sensor.name.as_deref()?)?;
    frameguin_model::sensor::ceiling(platform, kind)
}

fn own<'a>(sensor: &Sensor, thresholds: &'a [Thresholds]) -> Option<&'a Thresholds> {
    thresholds.iter().find(|own| own.index == sensor.index)
}

fn reachable(thresholds: &Thresholds, ceiling: Option<u16>) -> Thresholds {
    let Some(ceiling) = ceiling else {
        return *thresholds;
    };
    let within = |kelvin: Option<u16>| kelvin.filter(|kelvin| *kelvin <= ceiling);
    let fan_off = within(thresholds.fan_off);
    Thresholds {
        index: thresholds.index,
        warn: within(thresholds.warn),
        high: within(thresholds.high),
        halt: within(thresholds.halt),
        fan_off,
        // A ramp that starts within reach still runs, however far past the
        // ceiling its other end is set.
        fan_max: match thresholds.fan_off {
            Some(_) => thresholds.fan_max.filter(|_| fan_off.is_some()),
            None => within(thresholds.fan_max),
        },
    }
}

/// The scale every track of one board's sensors is laid on, from 20 °C to
/// 100 °C or to the highest any of them can reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scale {
    platform: Platform,
    end: u16,
}

impl Scale {
    #[must_use]
    pub fn of(platform: Platform, sensors: &[Sensor], thresholds: &[Thresholds]) -> Self {
        let end = sensors
            .iter()
            .filter_map(|sensor| {
                let own = own(sensor, thresholds)?;
                ceiling(platform, sensor).or_else(|| {
                    let ramp_end = own.fan_off.and(own.fan_max);
                    [own.warn, own.high, own.halt, ramp_end]
                        .into_iter()
                        .flatten()
                        .max()
                })
            })
            .fold(TRACK_END_KELVIN, u16::max);
        Self { platform, end }
    }

    fn at(self, kelvin: u16) -> u16 {
        let span = u32::from(self.end - TRACK_START_KELVIN);
        let past = u32::from(kelvin.clamp(TRACK_START_KELVIN, self.end) - TRACK_START_KELVIN);
        u16::try_from((past * 1000 + span / 2) / span).unwrap_or(1000)
    }

    /// None for a sensor with nothing to mark.
    #[must_use]
    pub fn track(
        self,
        sensor: &Sensor,
        thresholds: &[Thresholds],
        temperature: Temperature,
    ) -> Option<Track> {
        let ceiling = ceiling(self.platform, sensor);
        let reached = reachable(own(sensor, thresholds)?, ceiling);
        // The EC ramps the fan only for a sensor holding both ends. A ramp
        // set to end past the ceiling stops at it, the sensor reading no
        // higher.
        let ramp = reached
            .fan_off
            .zip(reached.fan_max)
            .map(|(off, max)| (off, ceiling.map_or(max, |ceiling| max.min(ceiling))));
        let cautions = [reached.warn, reached.high];
        let marks: Vec<u16> = reached
            .halt
            .or(ceiling)
            .into_iter()
            .chain(cautions.into_iter().rev().flatten())
            .chain(ramp.into_iter().flat_map(|(off, max)| [max, off]))
            .collect();
        if marks.is_empty() {
            return None;
        }

        let mut ticks: Vec<(u16, String)> = Vec::new();
        for kelvin in marks.into_iter().chain([self.end, TRACK_START_KELVIN]) {
            let position = self.at(kelvin);
            if ticks.iter().all(|(taken, _)| *taken != position) {
                ticks.push((position, degrees(kelvin).to_string()));
            }
        }

        Some(Track {
            ramp: ramp.map(|(off, max)| (self.at(off), self.at(max))),
            cautions: cautions
                .into_iter()
                .flatten()
                .map(|kelvin| self.at(kelvin))
                .collect(),
            shutdown: reached.halt.map(|kelvin| self.at(kelvin)),
            dot: match temperature {
                Temperature::Kelvin(kelvin) => Some(self.at(kelvin)),
                _ => None,
            },
            ticks,
            description: thresholds_label(&reached),
        })
    }
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
        Fan, FanDuty, Platform, Sensor, SensorReading, Temperature, ThermalLayout, ThermalState,
        Thresholds,
    };

    use super::{
        Scale, Track, celsius, fan_label, fan_name, sensor_name, temperature_label,
        thermal_summary, thresholds_label,
    };

    const AT_67: Temperature = Temperature::Kelvin(340);
    const DIE_READS_TO_100: Platform = Platform::Laptop13ProUltra3;
    const DIE_READS_TO_110: Platform = Platform::Laptop13Ultra1;

    fn sensor(index: u8, name: &str) -> Sensor {
        Sensor {
            index,
            name: Some(name.into()),
        }
    }

    fn thermistor() -> Sensor {
        sensor(0, "cpu_f75303@4d")
    }

    fn mainboard() -> Sensor {
        sensor(1, "local_f75397@4c")
    }

    fn battery() -> Sensor {
        sensor(2, "battery_temp@b")
    }

    fn die() -> Sensor {
        sensor(4, "peci-temp")
    }

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

    fn no_thresholds() -> Thresholds {
        Thresholds {
            index: 0,
            warn: None,
            high: None,
            halt: None,
            fan_off: None,
            fan_max: None,
        }
    }

    fn battery_thresholds() -> Thresholds {
        Thresholds {
            index: 2,
            warn: None,
            high: Some(323),
            halt: Some(333),
            fan_off: Some(313),
            fan_max: Some(323),
        }
    }

    fn die_thresholds() -> Thresholds {
        Thresholds {
            index: 4,
            warn: None,
            high: Some(393),
            halt: Some(400),
            fan_off: Some(376),
            fan_max: Some(378),
        }
    }

    fn mainboard_thresholds_to_127() -> Thresholds {
        Thresholds {
            index: 1,
            ..die_thresholds()
        }
    }

    fn ticks(track: &Track) -> Vec<&str> {
        track
            .ticks
            .iter()
            .map(|(_, label)| label.as_str())
            .collect()
    }

    fn lone_track(sensor: &Sensor, thresholds: Thresholds, at: Temperature) -> Option<Track> {
        let all = [thresholds];
        Scale::of(DIE_READS_TO_100, std::slice::from_ref(sensor), &all).track(sensor, &all, at)
    }

    #[test]
    fn a_track_places_each_threshold_on_its_scale() {
        let track = lone_track(&thermistor(), thresholds(), AT_67).unwrap();
        assert_eq!(track.ramp, Some((250, 688)));
        assert_eq!(track.cautions, [850]);
        assert_eq!(track.shutdown, Some(975));
        assert_eq!(track.dot, Some(588));
        assert_eq!(ticks(&track), ["98", "88", "75", "40", "100", "20"]);
        assert_eq!(
            track.description,
            "Fan 40–75 °C · Throttles at 88 °C · Shuts down at 98 °C"
        );
    }

    #[test]
    fn a_sensor_with_no_thresholds_has_no_track() {
        assert_eq!(lone_track(&thermistor(), no_thresholds(), AT_67), None);
        let scale = Scale::of(DIE_READS_TO_100, &[thermistor()], &[]);
        assert_eq!(scale.track(&thermistor(), &[], AT_67), None);
    }

    #[test]
    fn the_scale_ends_at_100_degrees_where_nothing_reaches_higher() {
        let all = [thresholds(), battery_thresholds()];
        let scale = Scale::of(DIE_READS_TO_100, &[thermistor(), battery()], &all);
        assert_eq!(scale.end, 373);
    }

    #[test]
    fn a_threshold_above_100_degrees_stretches_the_scale() {
        let all = [thresholds(), mainboard_thresholds_to_127()];
        let scale = Scale::of(DIE_READS_TO_100, &[thermistor(), mainboard()], &all);
        assert_eq!(scale.end, 400);
    }

    #[test]
    fn a_threshold_out_of_its_sensors_reach_does_not_stretch_the_scale() {
        let scale = Scale::of(DIE_READS_TO_100, &[die()], &[die_thresholds()]);
        assert_eq!(scale.end, 373);
    }

    #[test]
    fn a_ceiling_above_100_degrees_stretches_the_scale() {
        let scale = Scale::of(DIE_READS_TO_110, &[die()], &[die_thresholds()]);
        assert_eq!(scale.end, 383);
    }

    #[test]
    fn a_warn_threshold_is_marked_before_the_throttle_point() {
        let warned = Thresholds {
            warn: Some(358),
            ..thresholds()
        };
        let track = lone_track(&thermistor(), warned, AT_67).unwrap();
        assert_eq!(track.cautions, [813, 850]);
    }

    #[test]
    fn a_sensor_that_gave_no_temperature_has_no_dot() {
        let track = lone_track(&thermistor(), thresholds(), Temperature::Failed).unwrap();
        assert_eq!(track.dot, None);
    }

    #[test]
    fn a_temperature_off_the_scale_sits_at_its_end() {
        let at = |kelvin| lone_track(&thermistor(), thresholds(), Temperature::Kelvin(kelvin));
        assert_eq!(at(280).unwrap().dot, Some(0));
        assert_eq!(at(380).unwrap().dot, Some(1000));
    }

    #[test]
    fn a_lone_fan_threshold_draws_no_ramp() {
        let lone = Thresholds {
            fan_max: None,
            ..thresholds()
        };
        let track = lone_track(&thermistor(), lone, AT_67).unwrap();
        assert_eq!(track.ramp, None);
        assert_eq!(ticks(&track), ["98", "88", "100", "20"]);
    }

    #[test]
    fn thresholds_at_one_place_share_a_tick() {
        let track = lone_track(&battery(), battery_thresholds(), AT_67).unwrap();
        assert_eq!(ticks(&track), ["60", "50", "40", "100", "20"]);
    }

    #[test]
    fn thresholds_above_a_sensors_ceiling_are_neither_drawn_nor_described() {
        let track = lone_track(&die(), die_thresholds(), AT_67).unwrap();
        assert_eq!(track.ramp, None);
        assert_eq!(track.cautions, []);
        assert_eq!(track.shutdown, None);
        assert_eq!(track.dot, Some(588));
        assert_eq!(track.description, "");
    }

    #[test]
    fn a_ramp_set_to_end_past_the_ceiling_ends_at_it() {
        let straddling = Thresholds {
            high: Some(361),
            halt: Some(371),
            fan_off: Some(368),
            ..die_thresholds()
        };
        let all = [straddling, mainboard_thresholds_to_127()];
        let track = Scale::of(DIE_READS_TO_100, &[die(), mainboard()], &all)
            .track(&die(), &all, AT_67)
            .unwrap();
        assert_eq!(track.ramp, Some((701, 748)));
        assert_eq!(
            track.description,
            "Fan 95–105 °C · Throttles at 88 °C · Shuts down at 98 °C"
        );
    }

    #[test]
    fn a_sensor_with_no_shutdown_point_in_reach_is_ticked_at_its_ceiling() {
        let all = [die_thresholds(), mainboard_thresholds_to_127()];
        let track = Scale::of(DIE_READS_TO_100, &[die(), mainboard()], &all)
            .track(&die(), &all, AT_67)
            .unwrap();
        assert_eq!(ticks(&track), ["100", "127", "20"]);
    }

    #[test]
    fn a_sensor_with_a_shutdown_point_in_reach_gets_no_ceiling_tick() {
        let all = [Thresholds {
            index: 4,
            ..thresholds()
        }];
        let track = Scale::of(DIE_READS_TO_110, &[die()], &all)
            .track(&die(), &all, AT_67)
            .unwrap();
        assert_eq!(ticks(&track), ["98", "88", "75", "40", "110", "20"]);
    }

    #[test]
    fn ticks_come_most_severe_first_and_the_scales_ends_last() {
        let track = lone_track(&mainboard(), mainboard_thresholds_to_127(), AT_67).unwrap();
        assert_eq!(ticks(&track), ["127", "120", "105", "103", "20"]);
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
        let die = Sensor {
            index: 4,
            name: Some("peci-temp".into()),
        };
        let unmatched = Sensor {
            index: 2,
            name: Some("mystery_x@1".into()),
        };
        let unnamed = Sensor {
            index: 3,
            name: None,
        };
        assert_eq!(sensor_name(&placed), "Processor");
        assert_eq!(sensor_name(&die), "Processor die");
        assert_eq!(sensor_name(&unmatched), "mystery_x@1");
        assert_eq!(sensor_name(&unnamed), "Sensor 3");
    }

    fn fan(rpm: u16) -> Fan {
        Fan { index: 0, rpm }
    }

    fn duty(index: u8, percent: u8) -> FanDuty {
        FanDuty { index, percent }
    }

    #[test]
    fn a_fan_at_rest_reads_as_stopped() {
        assert_eq!(fan_label(&fan(2400), &[]), "2400 RPM");
        assert_eq!(fan_label(&fan(0), &[]), "Stopped");
        assert_eq!(fan_label(&fan(0), &[duty(0, 0)]), "Stopped");
    }

    #[test]
    fn a_fans_duty_follows_its_speed() {
        assert_eq!(fan_label(&fan(3200), &[duty(0, 45)]), "3200 RPM · 45%");
        assert_eq!(fan_label(&fan(0), &[duty(0, 19)]), "Stopped · 19%");
    }

    #[test]
    fn another_fans_duty_is_not_this_ones() {
        assert_eq!(fan_label(&fan(3200), &[duty(1, 45)]), "3200 RPM");
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
        assert_eq!(thresholds_label(&no_thresholds()), "");
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
        assert_eq!(thermal_summary(&layout, &state), "Processor die · 67 °C");
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
