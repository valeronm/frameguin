//! The chassis open switch and the input deck: their reads, and the words a
//! reader needs for them.

use std::rc::Rc;

use frameguin_wire::{
    ChassisControl, ChassisFeature, ChassisState, DeckState, DeviceResult as Result,
};

use super::present;

pub struct Chassis<C> {
    control: Rc<C>,
    features: Vec<ChassisFeature>,
}

impl<C: ChassisControl> Chassis<C> {
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.features().await)?.map(|features| Self {
            control: control.clone(),
            features,
        }))
    }

    #[must_use]
    pub fn has(&self, feature: ChassisFeature) -> bool {
        self.features.contains(&feature)
    }

    pub async fn read(&self) -> Result<ChassisState> {
        self.control.state().await
    }

    pub async fn deck_state(&self) -> Result<DeckState> {
        self.control.deck_state().await
    }
}

#[must_use]
pub fn deck_label(state: DeckState) -> &'static str {
    match state {
        DeckState::Off => "Off",
        DeckState::Disconnected => "Not detected",
        DeckState::TurningOn => "Detecting",
        DeckState::On => "On",
        DeckState::ForceOff => "Forced off",
        DeckState::ForceOn => "Forced on",
        DeckState::NoDetection => "On without detection",
    }
}

fn state_label(open: bool) -> &'static str {
    if open { "Open" } else { "Closed" }
}

/// On is the state of a deck detected as usual.
#[must_use]
pub fn chassis_summary(open: bool, deck: Option<DeckState>) -> String {
    match deck {
        Some(deck) if deck != DeckState::On => {
            format!("{} · Input deck {}", state_label(open), deck_inline(deck))
        }
        _ => state_label(open).to_owned(),
    }
}

/// [`deck_label`]'s words for the middle of a line.
fn deck_inline(state: DeckState) -> &'static str {
    match state {
        DeckState::Off => "off",
        DeckState::Disconnected => "not detected",
        DeckState::TurningOn => "detecting",
        DeckState::On => "on",
        DeckState::ForceOff => "forced off",
        DeckState::ForceOn => "forced on",
        DeckState::NoDetection => "on without detection",
    }
}

#[must_use]
pub fn open_now_label(open: bool) -> &'static str {
    super::yes_no(open)
}

/// The EC's counts stop at this value.
const COUNT_CEILING: u8 = 255;

#[must_use]
pub fn times_label(count: u8) -> String {
    if count == COUNT_CEILING {
        format!("{COUNT_CEILING} or more")
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{ChassisFeature, DeckState, DeviceError};

    use super::{Chassis, chassis_summary, deck_label, open_now_label, state_label, times_label};
    use crate::testing::{Machine, absent, ready};

    #[test]
    fn a_chassis_is_detected_with_its_features() {
        let chassis = ready(Chassis::detect(&Machine::new())).unwrap().unwrap();
        assert!(chassis.has(ChassisFeature::Deck));
        assert_eq!(ready(chassis.deck_state()), Ok(DeckState::On));
    }

    #[test]
    fn a_board_the_hardware_serves_no_chassis_for_is_absent() {
        let machine = Machine::failing(absent());
        assert!(ready(Chassis::detect(&machine)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_chassis() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(ready(Chassis::detect(&machine)).err(), Some(error));
    }

    #[test]
    fn the_summary_names_the_deck_only_when_it_is_not_on() {
        assert_eq!(chassis_summary(false, Some(DeckState::On)), "Closed");
        assert_eq!(chassis_summary(true, None), "Open");
        assert_eq!(
            chassis_summary(false, Some(DeckState::ForceOn)),
            "Closed · Input deck forced on"
        );
    }

    #[test]
    fn a_forced_deck_says_so() {
        assert_eq!(deck_label(DeckState::On), "On");
        assert_eq!(deck_label(DeckState::ForceOn), "Forced on");
        assert_eq!(deck_label(DeckState::Disconnected), "Not detected");
    }

    #[test]
    fn a_count_the_ec_stopped_at_reads_as_a_floor() {
        assert_eq!(times_label(254), "254");
        assert_eq!(times_label(255), "255 or more");
    }

    #[test]
    fn the_switch_is_worded_both_ways() {
        assert_eq!(state_label(true), "Open");
        assert_eq!(state_label(false), "Closed");
        assert_eq!(open_now_label(true), "Yes");
        assert_eq!(open_now_label(false), "No");
    }
}
