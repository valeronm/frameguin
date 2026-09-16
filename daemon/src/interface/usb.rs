//! `io.github.valeronm.Frameguin1.Usb`.

use frameguin_hardware::device::usb::Usb;
use frameguin_wire::{Attached, UsbControl};
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Usb")]
impl Served<Usb> {
    /// No polkit check: listing what is plugged in sets nothing.
    async fn get_attached(&self) -> fdo::Result<Vec<Attached>> {
        Ok(self.device().attached().await?)
    }
}
