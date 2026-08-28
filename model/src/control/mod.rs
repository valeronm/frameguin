//! One module per control: its detection, its read, its commands, its words.

pub mod touchpad;

use std::rc::Rc;

use frameguin_wire::{DeviceResult, TouchpadControl};

/// The controls this board has, each behind the one implementation of the
/// control traits. `None` is a control whose device answered for itself as
/// absent, and the front-ends gate on that.
pub struct Controls<C> {
    pub touchpad: Option<Rc<touchpad::Touchpad<C>>>,
}

impl<C: TouchpadControl> Controls<C> {
    /// Asks each control's device to detect itself. Fails only where the
    /// device could not be asked at all — an absent device is an answer, not
    /// a failure.
    pub async fn detect(control: &Rc<C>) -> DeviceResult<Self> {
        Ok(Self {
            touchpad: touchpad::Touchpad::detect(control).await?.map(Rc::new),
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.touchpad.is_none()
    }
}
