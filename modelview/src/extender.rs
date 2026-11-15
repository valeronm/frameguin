//! Framework's battery extender: what its reading is called.

use frameguin_contract::{ExtenderStage, ExtenderState};
use frameguin_model::control::battery::extender::{
    Effect, Reset, SECOND_STAGE_DAYS, Upcoming, in_force, stage_window,
};

use crate::battery::reading::percent_label;
use crate::words::{joined, span};

const SECONDS_PER_MINUTE: u32 = 60;
const SECONDS_PER_HOUR: u32 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u32 = 24 * SECONDS_PER_HOUR;

const NO_EFFECT: &str = "No effect";

/// A value and the line under it, which change together.
#[derive(PartialEq, Eq, Debug)]
pub struct Words {
    pub value: String,
    pub subtitle: Option<String>,
}

#[must_use]
pub fn stage_title(stage: ExtenderStage) -> &'static str {
    match stage {
        ExtenderStage::Inactive => "Not started",
        ExtenderStage::First => "Stage 1",
        ExtenderStage::Second => "Stage 2",
    }
}

#[must_use]
pub fn stage_value(stage: ExtenderStage) -> String {
    stage_window(stage).map_or_else(|| "Not lowered".to_owned(), window_label)
}

/// When a stage begins.
#[must_use]
pub fn stage_subtitle(stage: ExtenderStage, trigger_days: u16) -> String {
    match stage {
        ExtenderStage::Inactive => format!("For the first {}", days_label(trigger_days)),
        ExtenderStage::First => format!("After {}", days_label(trigger_days)),
        ExtenderStage::Second => format!("{} later", days_label(SECOND_STAGE_DAYS)),
    }
}

/// The stage in force, or that the extender is switched off.
#[must_use]
pub fn state_label(state: ExtenderState) -> &'static str {
    in_force(state).map_or("Off", stage_title)
}

#[must_use]
pub fn summary(state: ExtenderState, charge_limit: u8) -> String {
    let label = state_label(state);
    match Effect::of(state, charge_limit) {
        Effect::Holds(window) => joined([label, window_label(window).as_str()]),
        Effect::Covered => joined([label, "no effect"]),
        Effect::Off | Effect::NotStarted => label.to_owned(),
    }
}

/// What the extender does to the pack now, with the reason under it where
/// that is nothing; a switched-off extender gives none.
#[must_use]
pub fn effect(state: ExtenderState, charge_limit: u8) -> Words {
    let (value, reason) = match Effect::of(state, charge_limit) {
        Effect::Holds(window) => (
            format!("Holds the battery at {}", window_label(window)),
            None,
        ),
        Effect::Covered => (
            NO_EFFECT.to_owned(),
            Some("Held as low or lower by the charge limit"),
        ),
        Effect::NotStarted => (NO_EFFECT.to_owned(), Some("Begins with stage 1")),
        Effect::Off => (NO_EFFECT.to_owned(), None),
    };
    Words {
        value,
        subtitle: reason.map(str::to_owned),
    }
}

/// What the extender does next, and None for one switched off.
#[must_use]
pub fn next(state: ExtenderState, charger_connected: Option<bool>) -> Option<Words> {
    let upcoming = Upcoming::of(state)?;
    let wait = duration(u32::from(state.reset_minutes) * SECONDS_PER_MINUTE);
    let (when, setting) = match Reset::of(state.reset_seconds, charger_connected) {
        Reset::Counting(seconds) => (
            Some(format!("in {}", duration(seconds))),
            Some(format!("After {wait} off the charger")),
        ),
        Reset::Held => (Some(format!("after {wait} off the charger")), None),
        Reset::NeedsCharger => (
            Some(format!("after a charger is attached, then {wait} off it")),
            None,
        ),
        Reset::Unknown => (None, None),
    };
    let reset = |opening: &str| match &when {
        Some(when) => format!("{opening} {when}"),
        None => format!("Resets after {wait} off the charger"),
    };
    let (value, subtitle) = match upcoming {
        Upcoming::FirstStageIn(seconds) => (
            format!("Stage 1 in {}", duration(seconds)),
            Some(reset("Starts over")),
        ),
        Upcoming::SecondStage => (
            format!("Stage 2 within {}", days_label(SECOND_STAGE_DAYS)),
            Some(reset("Starts over")),
        ),
        Upcoming::Reset => (reset("Starts over"), setting),
        Upcoming::AwaitingReset => (reset("Countdown starts"), setting),
    };
    Some(Words { value, subtitle })
}

fn window_label((lower, upper): (u8, u8)) -> String {
    span(lower, percent_label(upper))
}

/// To the minute under a day, and to the hour from a day on.
fn duration(seconds: u32) -> String {
    let days = seconds / SECONDS_PER_DAY;
    let hours = seconds % SECONDS_PER_DAY / SECONDS_PER_HOUR;
    let minutes = seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    match (days, hours, minutes) {
        (0, 0, 0) => "under a minute".to_owned(),
        (0, 0, _) => format!("{minutes} min"),
        (0, _, _) => format!("{hours} h {minutes} min"),
        _ => format!("{} {hours} h", days_label(days)),
    }
}

fn days_label(days: impl Into<u32>) -> String {
    match days.into() {
        1 => "1 day".to_owned(),
        days => format!("{days} days"),
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{ExtenderStage, ExtenderState};
    use frameguin_model::control::battery::NO_CHARGE_LIMIT;
    use frameguin_model::control::battery::extender::STAGES;

    use super::{Words, effect, next, stage_subtitle, stage_title, stage_value, summary};

    const COUNTING: ExtenderState = ExtenderState {
        enabled: true,
        stage: ExtenderStage::Inactive,
        trigger_days: 5,
        first_stage_seconds: Some(3 * 86_400 + 4 * 3_600 + 59),
        reset_minutes: 30,
        reset_seconds: Some(30 * 60),
        held: None,
    };

    const FIRST: ExtenderState = ExtenderState {
        stage: ExtenderStage::First,
        first_stage_seconds: None,
        held: Some((90, 95)),
        ..COUNTING
    };

    const SECOND: ExtenderState = ExtenderState {
        stage: ExtenderStage::Second,
        held: Some((85, 87)),
        ..FIRST
    };

    const SECOND_AT_THE_LIMIT: ExtenderState = ExtenderState {
        held: Some((65, 70)),
        ..SECOND
    };

    const OFF: ExtenderState = ExtenderState {
        enabled: false,
        ..COUNTING
    };

    fn words(value: &str, subtitle: Option<&str>) -> Words {
        Words {
            value: value.to_owned(),
            subtitle: subtitle.map(str::to_owned),
        }
    }

    #[test]
    fn the_summary_adds_the_effect_of_a_stage_in_force() {
        assert_eq!(summary(SECOND, NO_CHARGE_LIMIT), "Stage 2 · 85–87%");
        assert_eq!(summary(SECOND_AT_THE_LIMIT, 70), "Stage 2 · no effect");
        assert_eq!(summary(COUNTING, NO_CHARGE_LIMIT), "Not started");
        assert_eq!(summary(OFF, NO_CHARGE_LIMIT), "Off");
    }

    #[test]
    fn a_window_kept_past_its_stage_follows_the_state_it_outlived() {
        let kept = ExtenderState {
            held: Some((85, 87)),
            ..COUNTING
        };
        assert_eq!(summary(kept, 90), "Not started · 85–87%");
        assert_eq!(effect(kept, 90), words("Holds the battery at 85–87%", None));
    }

    #[test]
    fn a_stage_in_effect_names_the_window_it_holds() {
        assert_eq!(
            effect(SECOND, NO_CHARGE_LIMIT),
            words("Holds the battery at 85–87%", None)
        );
    }

    #[test]
    fn no_effect_comes_with_its_reason_unless_switched_off() {
        assert_eq!(
            effect(SECOND_AT_THE_LIMIT, 70),
            words(
                "No effect",
                Some("Held as low or lower by the charge limit")
            )
        );
        assert_eq!(
            effect(COUNTING, NO_CHARGE_LIMIT),
            words("No effect", Some("Begins with stage 1"))
        );
        assert_eq!(effect(OFF, NO_CHARGE_LIMIT), words("No effect", None));
    }

    #[test]
    fn next_names_the_stage_ahead_over_what_starts_it_over() {
        let reset = Some("Starts over after 30 min off the charger");
        assert_eq!(
            next(COUNTING, Some(true)),
            Some(words("Stage 1 in 3 days 4 h", reset))
        );
        assert_eq!(
            next(FIRST, Some(true)),
            Some(words("Stage 2 within 2 days", reset))
        );
        assert_eq!(next(OFF, Some(true)), None);
    }

    #[test]
    fn a_reset_counting_down_shows_its_time_left_over_the_setting() {
        let unplugged = ExtenderState {
            reset_seconds: Some(12 * 60 + 30),
            ..SECOND
        };
        assert_eq!(
            next(unplugged, Some(false)),
            Some(words(
                "Starts over in 12 min",
                Some("After 30 min off the charger")
            ))
        );
        assert_eq!(
            next(
                ExtenderState {
                    stage: ExtenderStage::First,
                    ..unplugged
                },
                Some(false)
            ),
            Some(words(
                "Stage 2 within 2 days",
                Some("Starts over in 12 min")
            ))
        );
        assert_eq!(
            next(unplugged, Some(true)),
            Some(words("Starts over after 30 min off the charger", None))
        );
    }

    #[test]
    fn a_reset_with_no_battery_reading_names_only_the_setting() {
        assert_eq!(
            next(SECOND, None),
            Some(words("Resets after 30 min off the charger", None))
        );
        assert_eq!(
            next(FIRST, None),
            Some(words(
                "Stage 2 within 2 days",
                Some("Resets after 30 min off the charger")
            ))
        );
    }

    #[test]
    fn a_countdown_not_yet_started_names_the_reset_that_starts_it() {
        let unarmed = ExtenderState {
            first_stage_seconds: None,
            ..COUNTING
        };
        assert_eq!(
            next(unarmed, Some(true)),
            Some(words("Countdown starts after 30 min off the charger", None))
        );
        let unplugged = ExtenderState {
            reset_seconds: Some(12 * 60),
            ..unarmed
        };
        assert_eq!(
            next(unplugged, Some(false)),
            Some(words(
                "Countdown starts in 12 min",
                Some("After 30 min off the charger")
            ))
        );
    }

    #[test]
    fn a_reset_with_none_pending_off_the_charger_names_the_charger_first() {
        let spent = ExtenderState {
            reset_seconds: None,
            ..SECOND
        };
        assert_eq!(
            next(spent, Some(false)),
            Some(words(
                "Starts over after a charger is attached, then 30 min off it",
                None
            ))
        );
        let unarmed = ExtenderState {
            first_stage_seconds: None,
            reset_seconds: None,
            ..COUNTING
        };
        assert_eq!(
            next(unarmed, Some(false)),
            Some(words(
                "Countdown starts after a charger is attached, then 30 min off it",
                None
            ))
        );
        assert_eq!(
            next(
                ExtenderState {
                    reset_seconds: None,
                    ..FIRST
                },
                Some(false)
            ),
            Some(words(
                "Stage 2 within 2 days",
                Some("Starts over after a charger is attached, then 30 min off it")
            ))
        );
    }

    #[test]
    fn time_left_reads_to_the_minute_under_an_hour() {
        let at = |first_stage_seconds| {
            next(
                ExtenderState {
                    first_stage_seconds: Some(first_stage_seconds),
                    ..COUNTING
                },
                Some(true),
            )
            .unwrap()
            .value
        };
        assert_eq!(at(2 * 3_600 + 5 * 60), "Stage 1 in 2 h 5 min");
        assert_eq!(at(12 * 60 + 30), "Stage 1 in 12 min");
        assert_eq!(at(40), "Stage 1 in under a minute");
        assert_eq!(at(86_400), "Stage 1 in 1 day 0 h");
    }

    #[test]
    fn each_stage_is_titled_and_valued_by_its_window() {
        let rows = STAGES.map(|stage| (stage_title(stage), stage_value(stage)));
        assert_eq!(
            rows,
            [
                ("Not started", "Not lowered".to_owned()),
                ("Stage 1", "90–95%".to_owned()),
                ("Stage 2", "85–87%".to_owned()),
            ]
        );
    }

    #[test]
    fn each_stage_says_when_it_begins() {
        let begins = |stage| stage_subtitle(stage, 5);
        assert_eq!(begins(ExtenderStage::Inactive), "For the first 5 days");
        assert_eq!(begins(ExtenderStage::First), "After 5 days");
        assert_eq!(begins(ExtenderStage::Second), "2 days later");
    }
}
