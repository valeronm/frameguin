//! One module per control: its detection, its read, its commands, its words.

pub mod battery;
pub mod power_led;
pub mod touchpad;
pub mod touchscreen;

use std::rc::Rc;

use frameguin_wire::{
    BatteryControl, DeviceResult, PowerLedControl, TouchpadControl, TouchscreenControl,
};

/// The controls this board has, each behind the one implementation of the
/// control traits. `None` is a control whose device answered for itself as
/// absent, and the front-ends gate on that.
pub struct Controls<C> {
    pub battery: Option<Rc<battery::Battery<C>>>,
    pub touchpad: Option<Rc<touchpad::Touchpad<C>>>,
    pub touchscreen: Option<Rc<touchscreen::Touchscreen<C>>>,
    pub power_led: Option<Rc<power_led::PowerLed<C>>>,
}

impl<C: BatteryControl + TouchpadControl + TouchscreenControl + PowerLedControl> Controls<C> {
    /// Asks each control's device to detect itself. Fails only where the
    /// device could not be asked at all — an absent device is an answer, not
    /// a failure.
    pub async fn detect(control: &Rc<C>) -> DeviceResult<Self> {
        Ok(Self {
            battery: battery::Battery::detect(control).await?.map(Rc::new),
            touchpad: touchpad::Touchpad::detect(control).await?.map(Rc::new),
            touchscreen: touchscreen::Touchscreen::detect(control)
                .await?
                .map(Rc::new),
            power_led: power_led::PowerLed::detect(control).await?.map(Rc::new),
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.battery.is_none()
            && self.touchpad.is_none()
            && self.touchscreen.is_none()
            && self.power_led.is_none()
    }
}
