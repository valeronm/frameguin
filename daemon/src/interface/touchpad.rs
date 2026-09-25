//! `io.github.valeronm.Frameguin1.Touchpad`.

use frameguin_contract::{self as contract, TouchpadControl};
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Touchpad")]
impl Served<Touchpad> {
    async fn get_haptic_intensity(&self) -> fdo::Result<u8> {
        self.device().haptic_intensity().await.map_err(fdo_error)
    }

    async fn set_haptic_intensity(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.authorized(&header)
            .await?
            .set_haptic_intensity(percent)
            .await
            .map_err(fdo_error)
    }

    async fn get_click_force(&self) -> fdo::Result<contract::ClickForce> {
        self.device().click_force().await.map_err(fdo_error)
    }

    async fn set_click_force(
        &self,
        force: contract::ClickForce,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.authorized(&header)
            .await?
            .set_click_force(force)
            .await
            .map_err(fdo_error)
    }
}
