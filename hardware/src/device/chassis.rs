//! The chassis open switch, read and never set, and the input deck's power
//! state with the mode that decides it.

use std::sync::Arc;
use std::time::Duration;

use async_io::Timer;
use async_lock::Mutex;

use frameguin_contract::{
    ChassisControl, ChassisFeature, ChassisState, DeckMode, DeckState, DeviceError, DeviceResult,
};

use crate::ec::{ChassisEc, Ec};

/// `framework_tool --inputdeck-mode reset` holds the deck unpowered this long.
const RESET_HOLD: Duration = Duration::from_millis(100);

/// The EC polls for the deck every 10 ms and powers one it detects two
/// polls after detection resumes, reporting it absent until then.
const DETECTION: Duration = Duration::from_millis(50);

pub struct Chassis {
    ec: Arc<dyn ChassisEc>,
    features: Vec<ChassisFeature>,
    /// Held by every deck write, so none lands inside a reset's hold.
    deck_writes: Mutex<()>,
}

impl Chassis {
    pub(crate) fn detect(ec: &Arc<Ec>) -> Option<Self> {
        Self::new(ec.clone())
    }

    /// None where the EC refuses the read, which is how firmware without the
    /// chassis commands answers. The deck is probed by its getter's own read.
    pub fn new(ec: Arc<dyn ChassisEc>) -> Option<Self> {
        ec.chassis().ok()?;
        let mut features = Vec::new();
        if ec.deck_state().is_ok() {
            features.push(ChassisFeature::Deck);
        }
        Some(Self {
            ec,
            features,
            deck_writes: Mutex::new(()),
        })
    }
}

impl ChassisControl for Chassis {
    async fn state(&self) -> DeviceResult<ChassisState> {
        self.ec.chassis()
    }

    async fn features(&self) -> DeviceResult<Vec<ChassisFeature>> {
        Ok(self.features.clone())
    }

    async fn deck_state(&self) -> DeviceResult<DeckState> {
        self.ec.deck_state()
    }

    /// The first [`DeckMode::Automatic`] an EC is sent cuts the deck's power
    /// until detection finds it again.
    async fn set_deck_mode(&self, mode: DeckMode) -> DeviceResult<()> {
        let _writing = self.deck_writes.lock().await;
        if self.ec.deck_state()?.mode() == Some(mode) {
            return Ok(());
        }
        self.ec.set_deck_mode(mode)?;
        if mode == DeckMode::Automatic {
            Timer::after(DETECTION).await;
        }
        Ok(())
    }

    /// Forced off first so the [`DeckMode::Automatic`] after it always
    /// differs from the mode the EC holds.
    async fn reset_deck(&self) -> DeviceResult<()> {
        let _writing = self.deck_writes.lock().await;
        self.ec.set_deck_mode(DeckMode::ForceOff)?;
        Timer::after(RESET_HOLD).await;
        self.ec
            .set_deck_mode(DeckMode::Automatic)
            .or_else(|_| self.ec.set_deck_mode(DeckMode::Automatic))
            .map_err(|e| DeviceError::Failed(format!("the input deck is left forced off: {e}")))?;
        Timer::after(DETECTION).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use frameguin_contract::{ChassisControl, DeckMode, DeckState};

    use super::Chassis;
    use crate::testing::{Cover, ready};

    fn over(cover: Cover) -> (Chassis, Arc<Cover>) {
        let cover = Arc::new(cover);
        let chassis = Chassis::new(cover.clone()).expect("the EC answered");
        (chassis, cover)
    }

    fn written(cover: &Cover) -> Vec<DeckMode> {
        cover.modes_written.lock().unwrap().clone()
    }

    #[test]
    fn a_mode_the_state_already_answers_for_is_not_written() {
        let (chassis, cover) = over(Cover::default());
        ready(chassis.set_deck_mode(DeckMode::Automatic)).unwrap();
        assert_eq!(written(&cover), []);
    }

    #[test]
    fn a_forced_mode_is_written_once_and_read_back() {
        let (chassis, cover) = over(Cover::default());
        ready(chassis.set_deck_mode(DeckMode::ForceOn)).unwrap();
        ready(chassis.set_deck_mode(DeckMode::ForceOn)).unwrap();
        assert_eq!(written(&cover), [DeckMode::ForceOn]);
        assert_eq!(ready(chassis.deck_state()), Ok(DeckState::ForceOn));
    }

    #[test]
    fn a_state_no_mode_answers_for_is_written_over() {
        let (chassis, cover) = over(Cover {
            deck: Mutex::new(Some(DeckState::NoDetection)),
            ..Cover::default()
        });
        async_io::block_on(chassis.set_deck_mode(DeckMode::Automatic)).unwrap();
        assert_eq!(written(&cover), [DeckMode::Automatic]);
    }

    #[test]
    fn a_reset_cuts_the_deck_then_returns_it_to_detection() {
        let (chassis, cover) = over(Cover::default());
        async_io::block_on(chassis.reset_deck()).unwrap();
        assert_eq!(written(&cover), [DeckMode::ForceOff, DeckMode::Automatic]);
        assert_eq!(ready(chassis.deck_state()), Ok(DeckState::On));
    }

    #[test]
    fn a_reset_asks_again_for_a_return_to_detection_the_ec_refused() {
        let (chassis, cover) = over(Cover {
            refusing_automatic: Mutex::new(1),
            ..Cover::default()
        });
        async_io::block_on(chassis.reset_deck()).unwrap();
        assert_eq!(written(&cover), [DeckMode::ForceOff, DeckMode::Automatic]);
    }

    #[test]
    fn a_reset_that_cannot_return_the_deck_fails_with_it_forced_off() {
        let (chassis, cover) = over(Cover {
            refusing_automatic: Mutex::new(2),
            ..Cover::default()
        });
        assert!(async_io::block_on(chassis.reset_deck()).is_err());
        assert_eq!(written(&cover), [DeckMode::ForceOff]);
        assert_eq!(ready(chassis.deck_state()), Ok(DeckState::ForceOff));
    }

    #[test]
    fn an_ec_without_the_deck_command_offers_no_deck() {
        let ec = Cover {
            deck: Mutex::new(None),
            ..Cover::default()
        };
        let chassis = Chassis::new(Arc::new(ec)).expect("the EC answered");
        assert_eq!(ready(chassis.features()), Ok(Vec::new()));
    }

    #[test]
    fn an_ec_refusing_the_chassis_commands_has_no_chassis() {
        let ec = Cover {
            refusing: true,
            ..Cover::default()
        };
        assert!(Chassis::new(Arc::new(ec)).is_none());
    }
}
