//! The USB devices on the machine's root ports: one read. Which port a
//! device sits in is `crate::port`'s.

use std::rc::Rc;

use frameguin_contract::{Attached, DeviceResult as Result, UsbControl};

use super::present;

pub struct Usb<C> {
    control: Rc<C>,
}

impl<C: UsbControl> Usb<C> {
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.attached().await)?.map(|_| Self {
            control: control.clone(),
        }))
    }

    pub async fn read(&self) -> Result<Vec<Attached>> {
        self.control.attached().await
    }
}
