//! The app's end of each interface: a proxy per one, on the daemon's one
//! name and path.

use crate::vocabulary::{
    BUS_NAME, BatteryCondition, BatteryFeature, BatteryInfo, ClickForce, Identity, OBJECT_PATH,
    PortState, PowerLedLevel,
};

/// Any of the proxies below, on the daemon's one name and path.
///
/// # Errors
///
/// Building does no I/O and the name and path are constants, so only a
/// connection already closed fails here.
pub async fn proxy<P: zbus::proxy::ProxyImpl<'static> + From<zbus::Proxy<'static>>>(
    conn: &zbus::Connection,
) -> zbus::Result<P> {
    P::builder(conn)
        .destination(BUS_NAME)?
        .path(OBJECT_PATH)?
        .build()
        .await
}

// No default_service or default_path: they would restate BUS_NAME and
// OBJECT_PATH as literals the attribute can't read a const into, leaving two
// spellings of each with nothing checking they agree; [`proxy`] names them
// once.
#[zbus::proxy(interface = "io.github.valeronm.Frameguin1", gen_blocking = false)]
pub trait Frameguin {
    /// Every part detection found, mainboard first; fixed for the daemon's
    /// run.
    async fn get_devices(&self) -> zbus::Result<Vec<Identity>>;
    async fn get_build(&self) -> zbus::Result<(String, String)>;
}

/// The haptic touchpad, on its own interface at the same path. Absent from
/// the bus on a machine without one: the daemon registers a device's
/// interface only where it detected the device, so the interfaces at
/// [`OBJECT_PATH`] are the inventory.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.Touchpad",
    gen_blocking = false
)]
pub trait Touchpad {
    async fn get_haptic_intensity(&self) -> zbus::Result<u8>;
    async fn set_haptic_intensity(&self, percent: u8) -> zbus::Result<()>;
    async fn get_click_force(&self) -> zbus::Result<ClickForce>;
    async fn set_click_force(&self, force: ClickForce) -> zbus::Result<()>;
}

/// The touch panel, on its own interface at the same path and absent from
/// the bus where the daemon found no way to switch one.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.Touchscreen",
    gen_blocking = false
)]
pub trait Touchscreen {
    async fn get_enabled(&self) -> zbus::Result<bool>;
    async fn set_enabled(&self, enabled: bool) -> zbus::Result<()>;
}

/// The power button LED, on its own interface at the same path and absent
/// from the bus where the EC does not answer for one.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.PowerLed",
    gen_blocking = false
)]
pub trait PowerLed {
    async fn get_brightness(&self) -> zbus::Result<(u8, PowerLedLevel)>;
    async fn get_levels(&self) -> zbus::Result<Vec<PowerLedLevel>>;
    async fn set_level(&self, level: PowerLedLevel) -> zbus::Result<()>;
    async fn set_brightness(&self, percent: u8) -> zbus::Result<()>;
}

/// The battery, on its own interface at the same path and absent from the
/// bus where no pack answered in the EC's block.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.Battery",
    gen_blocking = false
)]
pub trait Battery {
    async fn get_info(&self) -> zbus::Result<BatteryInfo>;
    async fn get_condition(&self) -> zbus::Result<BatteryCondition>;
    async fn get_features(&self) -> zbus::Result<Vec<BatteryFeature>>;
    async fn get_charge_limit(&self) -> zbus::Result<u8>;
    async fn set_charge_limit(&self, percent: u8) -> zbus::Result<bool>;
    async fn get_charge_current_limit(&self) -> zbus::Result<u32>;
    async fn set_charge_current_limit(&self, milliamps: u32) -> zbus::Result<bool>;
}

/// The USB-C ports, on their own interface at the same path and absent from
/// the bus where the EC answers for no port.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.Ports",
    gen_blocking = false
)]
pub trait Ports {
    async fn get_ports(&self) -> zbus::Result<Vec<PortState>>;
}

/// Every device interface's proxy, dialled together: the one list of what
/// the daemon can serve, so a caller at either end cannot hold a shorter
/// one.
#[derive(Clone)]
pub struct Proxies {
    pub battery: BatteryProxy<'static>,
    pub touchpad: TouchpadProxy<'static>,
    pub touchscreen: TouchscreenProxy<'static>,
    pub power_led: PowerLedProxy<'static>,
    pub ports: PortsProxy<'static>,
}

impl Proxies {
    /// # Errors
    ///
    /// Only for a connection already closed, as [`proxy`].
    pub async fn dial(conn: &zbus::Connection) -> zbus::Result<Self> {
        Ok(Self {
            battery: proxy(conn).await?,
            touchpad: proxy(conn).await?,
            touchscreen: proxy(conn).await?,
            power_led: proxy(conn).await?,
            ports: proxy(conn).await?,
        })
    }
}
