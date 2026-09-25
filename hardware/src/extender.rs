//! What Framework's battery extender command carries — the read request and
//! the state it answers with — apart from [`crate::ec`] so the decoding is
//! testable without an EC.

use frameguin_contract::{ExtenderStage, ExtenderState};

/// `EC_CMD_BATTERY_EXTENDER`, one of Framework's own board commands.
pub(crate) const COMMAND: u16 = 0x3E24;

/// `ec_params_battery_extender` asking for a read: `disable`, `trigger_days`,
/// `reset_minutes` over two bytes, `cmd`, `manual`. A zero `cmd` is the
/// write, and a zero `disable` in it switches a disabled extender back on.
pub(crate) const READ_REQUEST: [u8; 6] = [0, 0, 0, 0, READ, 0];

const READ: u8 = 1;

const MICROS_PER_SECOND: u64 = 1_000_000;

/// The state out of `ec_response_battery_extender`, which is packed:
/// `current_stage`, `trigger_days` and `reset_minutes` over two bytes each,
/// `disable`, then `trigger_timedelta` and `reset_timedelta` in microseconds
/// over eight. None for an answer too short to hold it or a stage the
/// firmware does not define.
pub(crate) fn state(raw: &[u8]) -> Option<ExtenderState> {
    let word = |at: usize| Some(u16::from_le_bytes(raw.get(at..at + 2)?.try_into().ok()?));
    let trigger_micros = u64::from_le_bytes(raw.get(6..14)?.try_into().ok()?);
    Some(ExtenderState {
        enabled: *raw.get(5)? == 0,
        stage: match raw.first()? {
            0 => ExtenderStage::Inactive,
            1 => ExtenderStage::First,
            2 => ExtenderStage::Second,
            _ => return None,
        },
        trigger_days: word(1)?,
        first_stage_seconds: u32::try_from(trigger_micros / MICROS_PER_SECOND).unwrap_or(u32::MAX),
        reset_minutes: word(3)?,
    })
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{ExtenderStage, ExtenderState};

    use super::state;

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
            state(&answer(0, 0, THREE_DAYS_MICROS)),
            Some(ExtenderState {
                enabled: true,
                stage: ExtenderStage::Inactive,
                trigger_days: 5,
                first_stage_seconds: 3 * 86_400,
                reset_minutes: 30,
            })
        );
    }

    #[test]
    fn each_stage_and_the_switch_read_as_themselves() {
        let second = state(&answer(2, 0, 0)).unwrap();
        assert_eq!(second.stage, ExtenderStage::Second);
        assert_eq!(second.first_stage_seconds, 0);
        assert_eq!(state(&answer(1, 0, 0)).unwrap().stage, ExtenderStage::First);
        assert!(!state(&answer(0, 1, 0)).unwrap().enabled);
    }

    #[test]
    fn a_stage_the_firmware_does_not_define_is_no_state() {
        assert_eq!(state(&answer(3, 0, 0)), None);
    }

    #[test]
    fn a_short_answer_is_no_state() {
        assert_eq!(state(&answer(0, 0, 0)[..13]), None);
    }
}
