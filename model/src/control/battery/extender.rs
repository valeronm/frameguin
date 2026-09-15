//! Framework's battery extender: the window each stage holds a charged pack
//! in, and what its reading is called.

use frameguin_wire::{ExtenderStage, ExtenderState};

use super::reading::percent_label;

const SECONDS_PER_MINUTE: u32 = 60;
const SECONDS_PER_HOUR: u32 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u32 = 24 * SECONDS_PER_HOUR;

/// The stage and the window in force, a stage holding the lower of its own
/// window and the charge limit's. `charge_limit` is None on a board with no
/// charge limit, and 0 is the EC's own spelling of none.
#[must_use]
pub fn stage_label(state: ExtenderState, charge_limit: Option<u8>) -> String {
    if !state.enabled {
        return "Off".to_owned();
    }
    // The windows the EC's `battery_extender` sets for each stage.
    let (name, lower, upper) = match state.stage {
        ExtenderStage::Inactive => return "Waiting".to_owned(),
        ExtenderStage::First => ("Stage 1", 90, 95),
        ExtenderStage::Second => ("Stage 2", 85, 87),
    };
    match charge_limit {
        Some(limit) if limit != 0 && limit <= upper => format!(
            "{name} · no effect below the {} limit",
            percent_label(limit)
        ),
        _ => format!("{name} · {lower}–{upper}%"),
    }
}

/// How long until the first stage, and None where no countdown runs.
#[must_use]
pub fn first_stage_label(state: ExtenderState) -> Option<String> {
    (state.enabled && state.stage == ExtenderStage::Inactive && state.first_stage_seconds > 0)
        .then(|| duration(state.first_stage_seconds))
}

/// What the countdown to the first stage runs from.
#[must_use]
pub fn trigger_label(days: u16) -> String {
    format!("After {} without a reset", days_label(u32::from(days)))
}

#[must_use]
pub fn reset_label(minutes: u16) -> String {
    format!(
        "{} off the charger",
        duration(u32::from(minutes) * SECONDS_PER_MINUTE)
    )
}

/// To the minute under an hour, and to the hour past it.
fn duration(seconds: u32) -> String {
    let days = seconds / SECONDS_PER_DAY;
    let hours = seconds % SECONDS_PER_DAY / SECONDS_PER_HOUR;
    let minutes = seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    match (days, hours, minutes) {
        (0, 0, 0) => "Under a minute".to_owned(),
        (0, 0, _) => format!("{minutes} min"),
        (0, _, _) => format!("{hours} h {minutes} min"),
        _ => format!("{} {hours} h", days_label(days)),
    }
}

fn days_label(days: u32) -> String {
    if days == 1 {
        "1 day".to_owned()
    } else {
        format!("{days} days")
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{ExtenderStage, ExtenderState};

    use super::{first_stage_label, reset_label, stage_label, trigger_label};

    const COUNTING: ExtenderState = ExtenderState {
        enabled: true,
        stage: ExtenderStage::Inactive,
        trigger_days: 5,
        first_stage_seconds: 3 * 86_400 + 4 * 3_600 + 59,
        reset_minutes: 30,
    };

    const FIRST: ExtenderState = ExtenderState {
        stage: ExtenderStage::First,
        first_stage_seconds: 0,
        ..COUNTING
    };

    const SECOND: ExtenderState = ExtenderState {
        stage: ExtenderStage::Second,
        ..FIRST
    };

    const OFF: ExtenderState = ExtenderState {
        enabled: false,
        first_stage_seconds: 0,
        ..COUNTING
    };

    #[test]
    fn a_stage_under_no_charge_limit_names_its_own_window() {
        assert_eq!(stage_label(FIRST, Some(100)), "Stage 1 · 90–95%");
        assert_eq!(stage_label(SECOND, None), "Stage 2 · 85–87%");
        assert_eq!(stage_label(SECOND, Some(0)), "Stage 2 · 85–87%");
    }

    #[test]
    fn a_limit_at_or_under_the_stage_top_leaves_the_stage_no_effect() {
        assert_eq!(
            stage_label(FIRST, Some(80)),
            "Stage 1 · no effect below the 80% limit"
        );
        assert_eq!(
            stage_label(SECOND, Some(87)),
            "Stage 2 · no effect below the 87% limit"
        );
        assert_eq!(stage_label(SECOND, Some(88)), "Stage 2 · 85–87%");
    }

    #[test]
    fn before_a_stage_and_switched_off_name_no_window() {
        assert_eq!(stage_label(COUNTING, Some(100)), "Waiting");
        assert_eq!(stage_label(OFF, Some(100)), "Off");
    }

    #[test]
    fn only_a_running_countdown_has_time_left() {
        assert_eq!(first_stage_label(COUNTING).as_deref(), Some("3 days 4 h"));
        assert_eq!(first_stage_label(FIRST), None);
        assert_eq!(first_stage_label(OFF), None);
    }

    #[test]
    fn time_left_reads_to_the_minute_under_an_hour() {
        let at = |first_stage_seconds| {
            first_stage_label(ExtenderState {
                first_stage_seconds,
                ..COUNTING
            })
        };
        assert_eq!(at(2 * 3_600 + 5 * 60).as_deref(), Some("2 h 5 min"));
        assert_eq!(at(12 * 60 + 30).as_deref(), Some("12 min"));
        assert_eq!(at(40).as_deref(), Some("Under a minute"));
        assert_eq!(at(86_400).as_deref(), Some("1 day 0 h"));
    }

    #[test]
    fn the_settings_read_as_sentences() {
        assert_eq!(trigger_label(5), "After 5 days without a reset");
        assert_eq!(trigger_label(1), "After 1 day without a reset");
        assert_eq!(reset_label(30), "30 min off the charger");
    }
}
