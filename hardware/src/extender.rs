//! What Framework's battery extender command carries — the read request and
//! the state it answers with — and the sustainer window the charge control
//! command reads back, apart from [`crate::ec`] so the decoding is testable
//! without an EC.

use frameguin_contract::{DeviceError, DeviceResult, ExtenderStage, ExtenderState};

/// `EC_CMD_BATTERY_EXTENDER`, one of Framework's own board commands.
pub(crate) const COMMAND: u16 = 0x3E24;

/// `ec_params_battery_extender` asking for a read: `disable`, `trigger_days`,
/// `reset_minutes` over two bytes, `cmd`, `manual`. A zero `cmd` is the
/// write, and a zero `disable` in it switches a disabled extender back on.
pub(crate) const READ_REQUEST: [u8; 6] = [0, 0, 0, 0, 1, 0];

/// `EC_CMD_CHARGE_CONTROL`, whose get sub-command exists from version 2.
pub(crate) const SUSTAINER_COMMAND: u16 = 0x0096;
pub(crate) const SUSTAINER_VERSION: u8 = 2;

/// `ec_params_charge_control` asking for a read: `mode` over four bytes,
/// `cmd`, `flags`, then the sustainer's lower and upper bounds. A `cmd` of 1
/// is `EC_CHARGE_CONTROL_CMD_GET`.
pub(crate) const SUSTAINER_READ_REQUEST: [u8; 8] = [0, 0, 0, 0, 1, 0, 0, 0];

/// The window out of `ec_response_charge_control`, whose bounds follow
/// `mode` as signed bytes and are both -1 while the sustainer is off.
pub(crate) fn held(raw: &[u8]) -> DeviceResult<Option<(u8, u8)>> {
    let bounds: [u8; 2] = raw
        .get(4..6)
        .and_then(|bounds| bounds.try_into().ok())
        .ok_or_else(|| answered_no("sustainer window"))?;
    let [lower, upper] = bounds.map(|bound| u8::try_from(i8::from_le_bytes([bound])).ok());
    Ok(lower.zip(upper))
}

fn answered_no(what: &str) -> DeviceError {
    DeviceError::Failed(format!("the EC answered no {what}"))
}

const MICROS_PER_SECOND: u64 = 1_000_000;

/// The state out of `ec_response_battery_extender`, which is packed:
/// `current_stage`, `trigger_days` and `reset_minutes` over two bytes each,
/// `disable`, then `trigger_timedelta` and `reset_timedelta` in microseconds
/// over eight. Refused for an answer too short to hold it or a stage the
/// firmware does not define.
pub(crate) fn state(raw: &[u8], held: Option<(u8, u8)>) -> DeviceResult<ExtenderState> {
    decoded(raw, held).ok_or_else(|| answered_no("extender state"))
}

fn decoded(raw: &[u8], held: Option<(u8, u8)>) -> Option<ExtenderState> {
    let word = |at: usize| Some(u16::from_le_bytes(raw.get(at..at + 2)?.try_into().ok()?));
    let seconds = |at: usize| {
        let micros = u64::from_le_bytes(raw.get(at..at + 8)?.try_into().ok()?);
        // Rounded up, so the last second of a countdown is not read as none.
        let left = u32::try_from(micros.div_ceil(MICROS_PER_SECOND)).unwrap_or(u32::MAX);
        Some((micros > 0).then_some(left))
    };
    Some(ExtenderState {
        enabled: *raw.get(5)? == 0,
        stage: match raw.first()? {
            0 => ExtenderStage::Inactive,
            1 => ExtenderStage::First,
            2 => ExtenderStage::Second,
            _ => return None,
        },
        trigger_days: word(1)?,
        first_stage_seconds: seconds(6)?,
        reset_minutes: word(3)?,
        reset_seconds: seconds(14)?,
        held,
    })
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{ExtenderStage, ExtenderState};

    use super::{held, state};

    const THREE_DAYS_MICROS: u64 = 3 * 86_400 * 1_000_000;
    const THIRTY_MINUTES_MICROS: u64 = 30 * 60 * 1_000_000;

    fn answer(stage: u8, disable: u8, trigger_micros: u64) -> Vec<u8> {
        let mut raw = vec![stage];
        raw.extend_from_slice(&5u16.to_le_bytes());
        raw.extend_from_slice(&30u16.to_le_bytes());
        raw.push(disable);
        raw.extend_from_slice(&trigger_micros.to_le_bytes());
        raw.extend_from_slice(&THIRTY_MINUTES_MICROS.to_le_bytes());
        raw
    }

    #[test]
    fn a_counting_extender_reads_its_settings_and_the_time_left() {
        assert_eq!(
            state(&answer(0, 0, THREE_DAYS_MICROS), Some((65, 70))),
            Ok(ExtenderState {
                enabled: true,
                stage: ExtenderStage::Inactive,
                trigger_days: 5,
                first_stage_seconds: Some(3 * 86_400),
                reset_minutes: 30,
                reset_seconds: Some(30 * 60),
                held: Some((65, 70)),
            })
        );
    }

    #[test]
    fn each_stage_and_the_switch_read_as_themselves() {
        let second = state(&answer(2, 0, 0), None).unwrap();
        assert_eq!(second.stage, ExtenderStage::Second);
        assert_eq!(second.first_stage_seconds, None);
        assert_eq!(
            state(&answer(1, 0, 0), None).unwrap().stage,
            ExtenderStage::First
        );
        assert!(!state(&answer(0, 1, 0), None).unwrap().enabled);
    }

    #[test]
    fn the_last_second_of_a_countdown_is_still_time_left() {
        assert_eq!(
            state(&answer(0, 0, 1), None).unwrap().first_stage_seconds,
            Some(1)
        );
    }

    #[test]
    fn a_stage_the_firmware_does_not_define_is_no_state() {
        assert!(state(&answer(3, 0, 0), None).is_err());
    }

    #[test]
    fn a_short_answer_is_no_state() {
        assert!(state(&answer(0, 0, 0)[..21], None).is_err());
    }

    #[test]
    fn the_sustainer_window_reads_as_its_bounds_or_as_off() {
        assert_eq!(held(&[0, 0, 0, 0, 65, 70, 0, 0]), Ok(Some((65, 70))));
        assert_eq!(held(&[0, 0, 0, 0, 0xff, 0xff, 0, 0]), Ok(None));
        assert!(held(&[0, 0, 0, 0, 65]).is_err());
    }
}
