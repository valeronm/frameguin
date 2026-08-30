//! `io.github.valeronm.Frameguin1.Battery`.

use frameguin_hardware::device::battery::Battery;
use frameguin_wire::{self as wire, BatteryControl};
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Battery")]
impl Served<Battery> {
    async fn get_info(&self) -> fdo::Result<wire::BatteryInfo> {
        Ok(self.device().info().await?)
    }

    async fn get_condition(&self) -> fdo::Result<wire::BatteryCondition> {
        Ok(self.device().condition().await?)
    }

    async fn get_features(&self) -> fdo::Result<Vec<wire::BatteryFeature>> {
        Ok(self.device().features().await?)
    }

    async fn get_charge_limit(&self) -> fdo::Result<u8> {
        Ok(self.device().charge_limit().await?)
    }

    /// Skipped where the EC already holds the ceiling: nothing to write, and
    /// nothing worth an authorization prompt either. Asked of the hardware
    /// rather than the caller, so no client can act on a stale idea of it.
    async fn set_charge_limit(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<bool> {
        Battery::check_charge_limit(percent)?;
        let device = self.device();
        if device.charge_limit().await? == percent {
            return Ok(false);
        }
        self.authorize(&header).await?;
        Ok(device.set_charge_limit(percent).await?)
    }

    async fn get_charge_current_limit(&self) -> fdo::Result<u32> {
        Ok(self.device().charge_current_limit().await?)
    }

    /// Skipped as the ceiling is, except that the closest thing to the truth
    /// here is the device's own mirror, the EC having no readback to offer.
    async fn set_charge_current_limit(
        &self,
        milliamps: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<bool> {
        Battery::check_charge_current_limit(milliamps)?;
        let device = self.device();
        if device.charge_current_limit().await? == milliamps {
            return Ok(false);
        }
        self.authorize(&header).await?;
        Ok(device.set_charge_current_limit(milliamps).await?)
    }
}
