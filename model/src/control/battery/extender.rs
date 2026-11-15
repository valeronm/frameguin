//! Framework's battery extender: the window each stage holds a charged pack
//! in, what it does to the pack beside a charge limit, and what it does next.

use frameguin_contract::{ExtenderStage, ExtenderState};

use super::charge_limit_window;

/// Every stage, in the order the extender takes them.
pub const STAGES: [ExtenderStage; 3] = [
    ExtenderStage::Inactive,
    ExtenderStage::First,
    ExtenderStage::Second,
];

/// Days from the first stage to the second, fixed in the EC's
/// `battery_extender` and reported by no command.
pub const SECOND_STAGE_DAYS: u16 = 2;

/// The state-of-charge window the EC's `battery_extender` holds a stage at,
/// and None for the stage that holds nothing.
#[must_use]
pub const fn stage_window(stage: ExtenderStage) -> Option<(u8, u8)> {
    match stage {
        ExtenderStage::Inactive => None,
        ExtenderStage::First => Some((90, 95)),
        ExtenderStage::Second => Some((85, 87)),
    }
}

/// The stage in force, and None for an extender switched off.
#[must_use]
pub fn in_force(state: ExtenderState) -> Option<ExtenderStage> {
    state.enabled.then_some(state.stage)
}

/// What the extender does to a charged pack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    Off,
    NotStarted,
    /// The window it holds the pack in, which the EC can keep past the
    /// stage that set it.
    Holds((u8, u8)),
    /// A stage is in force and the pack is held no lower than the charge
    /// limit alone holds it.
    Covered,
}

impl Effect {
    /// The held window is the extender's wherever it is not the one the
    /// charge limit alone would hold.
    #[must_use]
    pub fn of(state: ExtenderState, charge_limit: u8) -> Self {
        match (state.held, in_force(state)) {
            (Some(held), _) if Some(held) != charge_limit_window(charge_limit) => Self::Holds(held),
            (_, None) => Self::Off,
            (_, Some(ExtenderStage::Inactive)) => Self::NotStarted,
            (_, Some(ExtenderStage::First | ExtenderStage::Second)) => Self::Covered,
        }
    }
}

/// What the extender does next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Upcoming {
    /// The first stage, in this many seconds.
    FirstStageIn(u32),
    /// A reset, which is what starts the countdown to the first stage.
    AwaitingReset,
    /// The second stage, within [`SECOND_STAGE_DAYS`].
    SecondStage,
    /// A reset, the last stage being in force.
    Reset,
}

impl Upcoming {
    /// None for an extender switched off.
    #[must_use]
    pub fn of(state: ExtenderState) -> Option<Self> {
        in_force(state).map(|current| match current {
            ExtenderStage::Inactive => state
                .first_stage_seconds
                .map_or(Self::AwaitingReset, Self::FirstStageIn),
            ExtenderStage::First => Self::SecondStage,
            ExtenderStage::Second => Self::Reset,
        })
    }
}

/// Where the reset that returns the extender to its start stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reset {
    /// Off the charger, with this many seconds left.
    Counting(u32),
    /// Held at the whole of `reset_minutes`, which the EC does on the
    /// charger.
    Held,
    /// Off the charger with none pending.
    NeedsCharger,
    /// Whether a charger is attached is not known.
    Unknown,
}

impl Reset {
    #[must_use]
    pub fn of(reset_seconds: Option<u32>, charger_connected: Option<bool>) -> Self {
        match (charger_connected, reset_seconds) {
            (Some(false), Some(seconds)) => Self::Counting(seconds),
            (Some(false), None) => Self::NeedsCharger,
            (Some(true), _) => Self::Held,
            (None, _) => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{ExtenderStage, ExtenderState};

    use super::{Effect, STAGES};
    use crate::control::battery::NO_CHARGE_LIMIT;

    const SECOND: ExtenderState = ExtenderState {
        enabled: true,
        stage: ExtenderStage::Second,
        trigger_days: 5,
        first_stage_seconds: None,
        reset_minutes: 30,
        reset_seconds: Some(30 * 60),
        held: Some((85, 87)),
    };

    const FIRST: ExtenderState = ExtenderState {
        stage: ExtenderStage::First,
        held: Some((90, 95)),
        ..SECOND
    };

    const NOT_STARTED: ExtenderState = ExtenderState {
        stage: ExtenderStage::Inactive,
        held: None,
        ..SECOND
    };

    const OFF: ExtenderState = ExtenderState {
        enabled: false,
        ..NOT_STARTED
    };

    #[test]
    fn the_stages_list_every_stage_in_declaration_order() {
        for (index, stage) in STAGES.into_iter().enumerate() {
            assert_eq!(stage as usize, index, "{stage:?}");
        }
        assert_eq!(STAGES.len(), ExtenderStage::Second as usize + 1);
    }

    #[test]
    fn a_window_the_charge_limit_would_not_hold_is_the_extenders() {
        assert_eq!(Effect::of(FIRST, NO_CHARGE_LIMIT), Effect::Holds((90, 95)));
        assert_eq!(Effect::of(SECOND, 95), Effect::Holds((85, 87)));
        let just_above = ExtenderState {
            held: Some((83, 87)),
            ..SECOND
        };
        assert_eq!(Effect::of(just_above, 88), Effect::Holds((83, 87)));
    }

    #[test]
    fn a_window_kept_past_its_stage_is_still_the_extenders() {
        let kept = ExtenderState {
            held: Some((85, 87)),
            ..NOT_STARTED
        };
        assert_eq!(Effect::of(kept, 90), Effect::Holds((85, 87)));
        let kept_off = ExtenderState {
            held: Some((85, 87)),
            ..OFF
        };
        assert_eq!(Effect::of(kept_off, 90), Effect::Holds((85, 87)));
    }

    #[test]
    fn a_stage_holding_the_charge_limits_own_window_is_covered() {
        let at_the_limit = ExtenderState {
            held: Some((65, 70)),
            ..SECOND
        };
        assert_eq!(Effect::of(at_the_limit, 70), Effect::Covered);
        let at_its_top = ExtenderState {
            held: Some((90, 95)),
            ..FIRST
        };
        assert_eq!(Effect::of(at_its_top, 95), Effect::Covered);
    }

    #[test]
    fn no_stage_in_force_leaves_the_charge_limits_window_alone() {
        let limited = ExtenderState {
            held: Some((75, 80)),
            ..NOT_STARTED
        };
        assert_eq!(Effect::of(limited, 80), Effect::NotStarted);
        assert_eq!(Effect::of(NOT_STARTED, NO_CHARGE_LIMIT), Effect::NotStarted);
        assert_eq!(Effect::of(OFF, NO_CHARGE_LIMIT), Effect::Off);
    }
}
