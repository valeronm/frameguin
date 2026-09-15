//! `io.github.valeronm.Frameguin1.Chassis`.

use frameguin_hardware::device::chassis::Chassis;
use frameguin_wire::{ChassisControl, ChassisFeature, ChassisState, DeckState};
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Chassis")]
impl Served<Chassis> {
    /// No polkit check: reading the switch sets nothing.
    async fn get_state(&self) -> fdo::Result<ChassisState> {
        Ok(self.device().state().await?)
    }

    async fn get_features(&self) -> fdo::Result<Vec<ChassisFeature>> {
        Ok(self.device().features().await?)
    }

    async fn get_deck_state(&self) -> fdo::Result<DeckState> {
        Ok(self.device().deck_state().await?)
    }
}
