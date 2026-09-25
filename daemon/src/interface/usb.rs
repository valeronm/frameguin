//! `io.github.valeronm.Frameguin1.Usb`.

use frameguin_contract::{Attached, UsbControl};
use frameguin_hardware::device::usb::Usb;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Usb")]
impl Served<Usb> {
    /// No polkit check: listing what is plugged in sets nothing.
    async fn get_attached(&self) -> fdo::Result<Vec<Attached>> {
        self.device().attached().await.map_err(fdo_error)
    }
}
