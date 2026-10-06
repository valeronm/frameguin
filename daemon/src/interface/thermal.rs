//! `io.github.valeronm.Frameguin1.Thermal`.

use frameguin_contract::{
    FanDuty, ThermalControl, ThermalFeature, ThermalLayout, ThermalState, Thresholds,
};
use frameguin_hardware::device::thermal::Thermal;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Thermal")]
impl Served<Thermal> {
    /// No polkit check: reading a sensor sets nothing.
    async fn get_features(&self) -> fdo::Result<Vec<ThermalFeature>> {
        self.device().features().await.map_err(fdo_error)
    }

    async fn get_layout(&self) -> fdo::Result<ThermalLayout> {
        self.device().layout().await.map_err(fdo_error)
    }

    async fn get_state(&self) -> fdo::Result<ThermalState> {
        self.device().state().await.map_err(fdo_error)
    }

    async fn get_thresholds(&self) -> fdo::Result<Vec<Thresholds>> {
        self.device().thresholds().await.map_err(fdo_error)
    }

    async fn get_fan_duties(&self) -> fdo::Result<Vec<FanDuty>> {
        self.device().fan_duties().await.map_err(fdo_error)
    }
}
