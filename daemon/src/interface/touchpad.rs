//! `io.github.valeronm.Frameguin1.Touchpad`.

use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_wire::{self as wire, TouchpadControl};
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Touchpad")]
impl Served<Touchpad> {
    async fn get_haptic_intensity(&self) -> fdo::Result<u8> {
        Ok(self.device().haptic_intensity().await?)
    }

    async fn set_haptic_intensity(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        Touchpad::check_haptic_intensity(percent)?;
        self.authorize(&header).await?;
        Ok(self.device().set_haptic_intensity(percent).await?)
    }

    async fn get_click_force(&self) -> fdo::Result<wire::ClickForce> {
        Ok(self.device().click_force().await?)
    }

    async fn set_click_force(
        &self,
        force: wire::ClickForce,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.authorize(&header).await?;
        Ok(self.device().set_click_force(force).await?)
    }
}
