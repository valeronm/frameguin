//! `io.github.valeronm.Frameguin1.Chassis`.

use frameguin_contract::{ChassisControl, ChassisFeature, ChassisState, DeckState};
use frameguin_hardware::device::chassis::Chassis;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Chassis")]
impl Served<Chassis> {
    /// No polkit check: reading the switch sets nothing.
    async fn get_state(&self) -> fdo::Result<ChassisState> {
        self.device().state().await.map_err(fdo_error)
    }

    async fn get_features(&self) -> fdo::Result<Vec<ChassisFeature>> {
        self.device().features().await.map_err(fdo_error)
    }

    async fn get_deck_state(&self) -> fdo::Result<DeckState> {
        self.device().deck_state().await.map_err(fdo_error)
    }
}
