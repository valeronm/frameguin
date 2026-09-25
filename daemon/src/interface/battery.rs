//! `io.github.valeronm.Frameguin1.Battery`.

use frameguin_contract::{self as contract, BatteryControl};
use frameguin_hardware::device::battery::Battery;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Battery")]
impl Served<Battery> {
    async fn get_info(&self) -> fdo::Result<contract::BatteryInfo> {
        self.device().info().await.map_err(fdo_error)
    }

    async fn get_condition(&self) -> fdo::Result<contract::BatteryCondition> {
        self.device().condition().await.map_err(fdo_error)
    }

    async fn get_features(&self) -> fdo::Result<Vec<contract::BatteryFeature>> {
        self.device().features().await.map_err(fdo_error)
    }

    async fn get_charge_limit(&self) -> fdo::Result<u8> {
        self.device().charge_limit().await.map_err(fdo_error)
    }

    async fn set_charge_limit(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<bool> {
        self.authorized(&header)
            .await?
            .set_charge_limit(percent)
            .await
            .map_err(fdo_error)
    }

    async fn get_charge_current_limit(&self) -> fdo::Result<contract::ChargeCurrentLimit> {
        self.device()
            .charge_current_limit()
            .await
            .map_err(fdo_error)
    }

    async fn set_charge_current_limit(
        &self,
        limit: contract::ChargeCurrentLimit,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<bool> {
        self.authorized(&header)
            .await?
            .set_charge_current_limit(limit)
            .await
            .map_err(fdo_error)
    }

    async fn get_extender(&self) -> fdo::Result<contract::ExtenderState> {
        self.device().extender().await.map_err(fdo_error)
    }
}
