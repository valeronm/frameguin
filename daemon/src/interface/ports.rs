//! `io.github.valeronm.Frameguin1.Ports`.

use frameguin_hardware::device::ports::Ports;
use frameguin_wire::{PortState, PortsControl};
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Ports")]
impl Served<Ports> {
    /// No polkit check and no header: reading what is plugged in sets
    /// nothing, and this interface has nothing that does.
    async fn get_ports(&self) -> fdo::Result<Vec<PortState>> {
        Ok(self.device().ports().await?)
    }
}
