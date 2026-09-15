//! `io.github.valeronm.Frameguin1.Chassis`.

use frameguin_hardware::device::chassis::Chassis;
use frameguin_wire::{ChassisControl, ChassisState};
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Chassis")]
impl Served<Chassis> {
    /// No polkit check: reading the switch sets nothing.
    async fn get_state(&self) -> fdo::Result<ChassisState> {
        Ok(self.device().state().await?)
    }
}
