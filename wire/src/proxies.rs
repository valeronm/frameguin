//! The app's end of each interface: a proxy per one, on the daemon's one
//! name and path.

use crate::vocabulary::{
    Attached, BUS_NAME, BatteryCondition, BatteryFeature, BatteryInfo, Board, ChargingLedFeature,
    ChargingLedSide, ChassisFeature, ChassisState, ClickForce, DeckState, ExtenderState, Identity,
    OBJECT_PATH, PortState, PowerLedLevel, PrivacyState,
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
    /// The machine the daemon runs on; fixed for its run.
    async fn get_board(&self) -> zbus::Result<Board>;
    async fn get_build(&self) -> zbus::Result<(String, String)>;
    /// Whether the daemon writes back what each control was last set to
    /// after a boot or a resume.
    async fn get_restore(&self) -> zbus::Result<bool>;
    async fn set_restore(&self, enabled: bool) -> zbus::Result<()>;
    /// Writes them back now; nothing while the switch is off.
    async fn restore(&self) -> zbus::Result<()>;
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

/// Absent from the bus where the kernel has no node for the charging LED that
/// it could hand back to the EC.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.ChargingLed",
    gen_blocking = false
)]
pub trait ChargingLed {
    async fn get_enabled(&self) -> zbus::Result<bool>;
    async fn set_enabled(&self, enabled: bool) -> zbus::Result<()>;
    async fn get_features(&self) -> zbus::Result<Vec<ChargingLedFeature>>;
    async fn get_side(&self) -> zbus::Result<ChargingLedSide>;
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
    async fn get_extender(&self) -> zbus::Result<ExtenderState>;
}

/// The USB-C ports, on their own interface at the same path and absent from
/// the bus where the EC answers for no port.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.Ports",
    gen_blocking = false
)]
pub trait Ports {
    async fn get_ports(&self, controller_ports: u8) -> zbus::Result<Vec<PortState>>;
}

/// Absent from the bus where the EC does not answer the chassis commands.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.Chassis",
    gen_blocking = false
)]
pub trait Chassis {
    async fn get_state(&self) -> zbus::Result<ChassisState>;
    async fn get_features(&self) -> zbus::Result<Vec<ChassisFeature>>;
    async fn get_deck_state(&self) -> zbus::Result<DeckState>;
}

/// Absent from the bus where the EC does not answer for the privacy switches.
#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1.PrivacySwitches",
    gen_blocking = false
)]
pub trait PrivacySwitches {
    async fn get_switches(&self) -> zbus::Result<PrivacyState>;
}

/// Absent from the bus on a machine that is not a Framework one, or whose USB
/// bus cannot be listed.
#[zbus::proxy(interface = "io.github.valeronm.Frameguin1.Usb", gen_blocking = false)]
pub trait Usb {
    async fn get_attached(&self) -> zbus::Result<Vec<Attached>>;
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
    pub charging_led: ChargingLedProxy<'static>,
    pub ports: PortsProxy<'static>,
    pub chassis: ChassisProxy<'static>,
    pub privacy_switches: PrivacySwitchesProxy<'static>,
    pub usb: UsbProxy<'static>,
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
            charging_led: proxy(conn).await?,
            ports: proxy(conn).await?,
            chassis: proxy(conn).await?,
            privacy_switches: proxy(conn).await?,
            usb: proxy(conn).await?,
        })
    }
}
