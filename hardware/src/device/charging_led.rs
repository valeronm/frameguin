//! The charging LED: the EC's policy or the kernel holding it dark, and which
//! side of the chassis is lit.
//!
//! Nothing here is restorable: a reboot leaves the LED on, and a resume
//! leaves it as it was.

use std::sync::Arc;
use std::time::Duration;

use async_io::Timer;
use frameguin_wire::{
    ChargingLedControl, ChargingLedFeature, ChargingLedSide, DeviceError, DeviceResult,
};

use crate::ec::{Ec, SideEnables};
use crate::led::{self, LedClass};

/// The EC re-evaluates its LED policy every 200 ms, plus margin.
const POLICY_TICK: Duration = Duration::from_millis(250);

pub struct ChargingLed {
    leds: Box<dyn LedClass>,
    sides: Option<Arc<dyn SideEnables>>,
}

impl ChargingLed {
    pub(crate) fn detect(ec: &Arc<Ec>) -> Option<Self> {
        Self::new(Box::new(led::Sysfs::charging()), ec.clone())
    }

    /// None where the kernel has no node it could both darken and hand back.
    /// The sides are probed by their getter's own read.
    pub fn new(leds: Box<dyn LedClass>, ec: Arc<dyn SideEnables>) -> Option<Self> {
        leds.controllable()?;
        let sides = ec.side_enables().is_ok().then_some(ec);
        Some(Self { leds, sides })
    }
}

impl ChargingLedControl for ChargingLed {
    async fn enabled(&self) -> DeviceResult<bool> {
        Ok(self.leds.held_dark().is_none())
    }

    /// Returns once the EC's policy has had a tick to drive the LED it was
    /// handed, so a read of the side that follows sees the side it lit.
    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        if self.enabled().await? == enabled {
            return Ok(());
        }
        if !enabled {
            return self.leds.hold_dark();
        }
        self.leds.release_held()?;
        Timer::after(POLICY_TICK).await;
        Ok(())
    }

    async fn features(&self) -> DeviceResult<Vec<ChargingLedFeature>> {
        Ok(if self.sides.is_some() {
            vec![ChargingLedFeature::Side]
        } else {
            Vec::new()
        })
    }

    /// The EC raises both enables whenever its policy is not driving the LED.
    async fn side(&self) -> DeviceResult<ChargingLedSide> {
        let sides = self.sides.as_ref().ok_or_else(|| {
            DeviceError::NotSupported("the EC does not name the charging LED's sides".into())
        })?;
        if self.leds.held_dark().is_some() {
            return Ok(ChargingLedSide::Neither);
        }
        Ok(match sides.side_enables()? {
            (false, false) => ChargingLedSide::Neither,
            (true, false) => ChargingLedSide::Left,
            (false, true) => ChargingLedSide::Right,
            (true, true) => ChargingLedSide::Both,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_wire::{ChargingLedControl, ChargingLedFeature, ChargingLedSide, DeviceError};

    use super::ChargingLed;
    use crate::testing::{Leds, Log, Sides, ready, writes};

    struct Bench {
        led: ChargingLed,
        log: Log,
    }

    fn over(sides: Sides) -> Bench {
        let log = Log::default();
        let leds = Leds {
            log: log.clone(),
            ..Leds::default()
        };
        let led = ChargingLed::new(Box::new(leds), Arc::new(sides)).expect("the kernel has a node");
        Bench { led, log }
    }

    fn fresh() -> Bench {
        over(Sides::default())
    }

    #[test]
    fn a_kernel_without_a_node_has_no_charging_led() {
        let leds = Leds {
            node: None,
            ..Leds::default()
        };
        assert!(ChargingLed::new(Box::new(leds), Arc::new(Sides::default())).is_none());
    }

    #[test]
    fn off_darkens_through_the_kernel_and_on_hands_it_back() {
        let Bench { led, log } = fresh();
        assert_eq!(ready(led.enabled()), Ok(true));
        ready(led.set_enabled(false)).unwrap();
        assert_eq!(ready(led.enabled()), Ok(false));
        async_io::block_on(led.set_enabled(true)).unwrap();
        assert_eq!(ready(led.enabled()), Ok(true));
        assert_eq!(writes(&log), ["darken", "release"]);
    }

    #[test]
    fn on_leaves_a_led_nobody_holds_dark_alone() {
        let Bench { led, log } = fresh();
        ready(led.set_enabled(true)).unwrap();
        assert!(writes(&log).is_empty());
    }

    #[test]
    fn off_leaves_a_led_already_held_dark_alone() {
        let Bench { led, log } = fresh();
        ready(led.set_enabled(false)).unwrap();
        ready(led.set_enabled(false)).unwrap();
        assert_eq!(writes(&log), ["darken"]);
    }

    #[test]
    fn the_lit_side_is_the_ecs_enable() {
        let Bench { led, .. } = fresh();
        assert_eq!(ready(led.features()), Ok(vec![ChargingLedFeature::Side]));
        assert_eq!(ready(led.side()), Ok(ChargingLedSide::Left));
    }

    #[test]
    fn a_led_held_dark_is_lit_on_neither_side_whatever_the_enables_say() {
        let Bench { led, .. } = over(Sides {
            enables: Some((true, true)),
        });
        assert_eq!(ready(led.side()), Ok(ChargingLedSide::Both));
        ready(led.set_enabled(false)).unwrap();
        assert_eq!(ready(led.side()), Ok(ChargingLedSide::Neither));
    }

    #[test]
    fn an_ec_refusing_the_pins_offers_no_side() {
        let Bench { led, .. } = over(Sides { enables: None });
        assert_eq!(ready(led.features()), Ok(Vec::new()));
        assert!(matches!(
            ready(led.side()),
            Err(DeviceError::NotSupported(_))
        ));
        assert_eq!(ready(led.enabled()), Ok(true));
    }
}
