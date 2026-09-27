//! What the EC's memmap thermal bytes and its threshold command mean, apart
//! from `ec.rs` so the decoding is testable without an EC.

use frameguin_contract::{Temperature, Thresholds};

pub(crate) const TEMP_SENSOR: u16 = 0x00;
pub(crate) const TEMP_SENSOR_ENTRIES: u16 = 16;
pub(crate) const FAN: u16 = 0x10;
pub(crate) const FAN_ENTRIES: u16 = 4;
pub(crate) const TEMP_SENSOR_B: u16 = 0x18;
pub(crate) const TEMP_SENSOR_B_ENTRIES: u16 = 8;
pub(crate) const THERMAL_VERSION: u16 = 0x23;
/// The memmap thermal version that added the second sensor range.
pub(crate) const SECOND_RANGE_VERSION: u8 = 2;

const NOT_PRESENT: u8 = 0xff;
const ERROR: u8 = 0xfe;
const NOT_POWERED: u8 = 0xfd;
const NOT_CALIBRATED: u8 = 0xfc;
/// The memmap stores kelvin less this, to fit a byte.
const OFFSET: u16 = 200;
const FAN_NOT_PRESENT: u16 = 0xffff;
/// Older firmware's word for a stall, which current firmware reports as 0.
const FAN_STALLED_DEPRECATED: u16 = 0xfffe;

/// A sensor's thresholds as `EC_CMD_THERMAL_GET_THRESHOLD` v1 holds them, in
/// kelvin: `host` is warn, high and halt in that order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RawThresholds {
    pub host: [u32; 3],
    pub fan_off: u32,
    pub fan_max: u32,
}

/// None for a sensor the EC reports not present.
pub(crate) fn temperature(byte: u8) -> Option<Temperature> {
    match byte {
        NOT_PRESENT => None,
        ERROR => Some(Temperature::Failed),
        NOT_POWERED => Some(Temperature::Unpowered),
        NOT_CALIBRATED => Some(Temperature::Uncalibrated),
        stored => Some(Temperature::Kelvin(u16::from(stored) + OFFSET)),
    }
}

/// None for a fan slot the EC reports empty.
pub(crate) fn fan_rpm(word: u16) -> Option<u16> {
    match word {
        FAN_NOT_PRESENT => None,
        FAN_STALLED_DEPRECATED => Some(0),
        rpm => Some(rpm),
    }
}

/// 0 disables a threshold.
fn threshold(kelvin: u32) -> Option<u16> {
    u16::try_from(kelvin).ok().filter(|kelvin| *kelvin != 0)
}

pub(crate) fn thresholds(index: u8, raw: RawThresholds) -> Thresholds {
    let [warn, high, halt] = raw.host;
    Thresholds {
        index,
        warn: threshold(warn),
        high: threshold(high),
        halt: threshold(halt),
        fan_off: threshold(raw.fan_off),
        fan_max: threshold(raw.fan_max),
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{Temperature, Thresholds};

    use super::{RawThresholds, fan_rpm, temperature, thresholds};

    #[test]
    fn a_stored_byte_is_kelvin_past_the_offset() {
        assert_eq!(temperature(140), Some(Temperature::Kelvin(340)));
        assert_eq!(temperature(0), Some(Temperature::Kelvin(200)));
        assert_eq!(temperature(0xfb), Some(Temperature::Kelvin(451)));
    }

    #[test]
    fn the_reserved_bytes_are_states_and_absence() {
        assert_eq!(temperature(0xff), None);
        assert_eq!(temperature(0xfe), Some(Temperature::Failed));
        assert_eq!(temperature(0xfd), Some(Temperature::Unpowered));
        assert_eq!(temperature(0xfc), Some(Temperature::Uncalibrated));
    }

    #[test]
    fn a_fan_word_is_its_speed_and_both_stall_forms_read_as_stopped() {
        assert_eq!(fan_rpm(2400), Some(2400));
        assert_eq!(fan_rpm(0), Some(0));
        assert_eq!(fan_rpm(0xfffe), Some(0));
        assert_eq!(fan_rpm(0xffff), None);
    }

    #[test]
    fn a_zero_threshold_is_disabled() {
        let raw = RawThresholds {
            host: [0, 361, 371],
            fan_off: 313,
            fan_max: 348,
        };
        assert_eq!(
            thresholds(1, raw),
            Thresholds {
                index: 1,
                warn: None,
                high: Some(361),
                halt: Some(371),
                fan_off: Some(313),
                fan_max: Some(348),
            }
        );
    }

    #[test]
    fn a_threshold_past_any_sensor_is_not_one() {
        let raw = RawThresholds {
            host: [u32::MAX, 0, 0],
            fan_off: 0,
            fan_max: 0,
        };
        assert_eq!(thresholds(0, raw).warn, None);
    }
}
