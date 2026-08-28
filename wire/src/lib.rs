//! The vocabulary of `io.github.valeronm.Frameguin1`, declared once.
//!
//! The daemon's `#[interface]` impl and the app's calls are two independent
//! restatements of the same interface that meet only at runtime, in the bus:
//! a name or a signature they disagree about comes back as an error reply,
//! and a *string* they disagree about — a capability, a level, a click force
//! — comes back as a value that is well-formed and meaningless. Naming those
//! strings as types is what moves that second class of drift to compile time.
//! A number both ends must simply agree on belongs here for the same reason,
//! and fails more quietly still: the receiver accepts it and acts on it.
//!
//! Every enum here serializes as `s`, so the wire format is the plain string
//! the variant is named after.

use serde::{Deserialize, Serialize};
use zbus::zvariant::Type;

pub const BUS_NAME: &str = "io.github.valeronm.Frameguin";
pub const OBJECT_PATH: &str = "/io/github/valeronm/Frameguin";

/// The DMI `sys_vendor` of the hardware this is for. Both ends test it and
/// neither can see the other's answer: the daemon gates opening the EC on it,
/// the app titles its window from it, and a pair that disagreed would either
/// name a board whose every control errors or deny one that works. A string
/// both ends must agree on, like the haptic steps below — reading it is each
/// end's own business, spelling it is not.
pub const VENDOR: &str = "Framework";

/// Charge as fast as the battery asks. The EC clamps every requested charge
/// current against its limit, so the largest value is the one that imposes
/// none; 0 at the other end would mean never charge, which no setter accepts.
pub const NO_CHARGE_CURRENT_LIMIT: u32 = u32::MAX;

/// Every intensity `SetHapticIntensity` accepts. The touchpad firmware
/// implements five steps rather than the 0-100 its HID descriptor advertises,
/// and this is the one control whose legal arguments the app cannot look up
/// for itself — the crate that knows them is the one it must not link.
pub const HAPTIC_INTENSITY_LEVELS: [u8; 5] = [0, 25, 50, 75, 100];

/// What a board supports, per the daemon's probe: one name per exposed
/// operation, never per subsystem.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// A pack the EC's memmap block answers for, and so [`BatteryInfo`] — the
    /// one reading here that no setter pairs with. Named for the pack rather
    /// than for a method, because what a board either has or hasn't is the
    /// battery; which calls that makes answerable is this end's business.
    Battery,
    ChargeLimit,
    ChargeCurrentLimit,
    KeyboardBacklight,
    PowerLedBrightness,
    /// V1 of the EC's `FP_LED` brightness command: the raw percentage write,
    /// and with it the ultra-low and auto levels (framework-system issue
    /// #211).
    PowerLedBrightnessCustom,
    /// Darkening the LED, which the EC's command cannot express: it takes
    /// 1-100 and reserves 0, refusing to extinguish the indicator. Off is
    /// reached by taking the LED off the EC's own policy through the kernel's
    /// LED class, so this capability answers for that interface rather than
    /// for a command version.
    PowerLedOff,
    /// What the pack says about itself past the EC's summary of it — its
    /// temperature, its cell voltages, and the alarms it is raising. One name
    /// for all three: they are the same device over the same transport,
    /// reached by the same passthrough, so a board answering for one answers
    /// for the others.
    BatteryCondition,
    /// Switching the touch panel off. Named for the panel rather than for the
    /// way it is reached, which differs by machine and is the daemon's
    /// business alone: this end asks whether touch can be switched, never how.
    /// Offered wherever one of those ways is within reach — which on some
    /// machines takes two facts about separately sold parts, a panel being
    /// fitted and this daemon knowing that board's way of gating one, and on
    /// others takes only the panel, its own firmware being what switches it.
    Touchscreen,
}

/// Something wrong with the pack, as the pack itself judges it — named rather
/// than left as the bit it is decoded from, the same reason [`ChargeFlow`] is
/// a name.
///
/// Only the two the gauge raises for a fault. Its status word carries four
/// more that read like warnings and are not: the terminate-charge and
/// terminate-discharge alarms are how a pack asks for charging or discharging
/// to end, which it does at every full charge and every empty one — the
/// datasheet counts "valid charge terminations" as a lifetime statistic. The
/// remaining-time and remaining-capacity alarms fire against thresholds the
/// host sets, which on a laptop is the desktop's job and not this app's. And
/// the fully-charged, fully-discharged, discharging and initialized bits are
/// states rather than alarms — the EC's own console prints them as a separate
/// group, and the charge percentage says all four better.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum BatteryAlarm {
    /// Charged past what the pack considers safe.
    OverCharged,
    OverTemperature,
    /// The pack asking that charging *and* discharging both stop, which it
    /// does for a safety alert, a permanent failure, or a pack reporting
    /// itself absent.
    ///
    /// Derived from two bits rather than read from one, and sound because the
    /// two cannot both be raised by ordinary operation: each is set routinely
    /// only from the gauge's own termination logic, one of which requires the
    /// pack to be charging and the other to be discharging. Together they
    /// leave no reading but a fault — and they are the only sight this
    /// interface has of the over-current and under-voltage faults, which the
    /// two alarms above do not cover.
    SafetyFault,
}

/// What the pack reports about itself that the EC's block does not carry.
///
/// Every part of it comes from the pack over the EC's I2C passthrough, in one
/// call because one reader wants them together and each transfer is a message
/// to a device the EC is also driving.
#[derive(Serialize, Deserialize, Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct BatteryCondition {
    /// Each cell's terminal voltage in mV, in the order the pack numbers them.
    /// What these are worth is the spread between them: the EC publishes only
    /// the pack total, which stays healthy-looking while one cell drifts.
    pub cell_millivolts: Vec<u32>,
    /// Empty on a pack raising none, which is the ordinary case.
    pub alarms: Vec<BatteryAlarm>,
    /// The pack's own temperature in tenths of a degree Celsius, which is the
    /// resolution its sensor works in — whole degrees would be this end
    /// rounding away what the pack measured. The EC polls this same sensor and
    /// republishes it whole into its thermal array; read here first-hand, it
    /// is current rather than last-polled, and answers on boards whose EC does
    /// not relay it.
    pub decicelsius: i16,
}

/// What the pack is doing, which is not the same question as what the EC's
/// battery flags answer: the EC's discharging flag is set whenever the pack
/// is not being charged, a full battery on a connected charger included.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum ChargeFlow {
    Charging,
    /// Running the machine, which is what a pack does with no charger
    /// attached.
    Discharging,
    /// A charger attached and nothing going into the pack — where one held
    /// at its ceiling, or simply full, rests.
    Idle,
}

/// What the EC says about the pack right now. The direction arrives as a
/// name rather than as the flag byte it is decoded from, for the reason
/// every other vocabulary here is a name: the process that must not link the
/// EC library has no business knowing its bit layout.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct BatteryState {
    /// Charge as a share of the pack's last full charge, which is what the
    /// EC measures it against — so it reaches 100% on a pack whose capacity
    /// has faded well below its design one.
    pub percent: u8,
    pub flow: ChargeFlow,
    /// How fast charge is moving, in mA, and 0 when nothing is. Unsigned in
    /// both directions — `flow` is what gives it a sign.
    pub milliamps: u32,
    /// The pack's terminal voltage in mV, as read in the same moment as the
    /// rate. It sags under load and rises towards the end of a charge, so the
    /// power a rate carries has to be taken against this reading rather than
    /// against the pack's nominal voltage.
    pub millivolts: u32,
}

/// Everything the EC's memmap battery block says about the pack, for a reader
/// looking at the pack itself rather than at the controls that shape it.
/// The block is fetched whole or not at all, and [`BatteryState`] — its moving
/// part — is what a caller showing only a charge takes out of it. Carried as
/// that struct rather than restated as fields, so a report and the row above
/// it cannot come from two different moments.
///
/// What the pack says about *itself* is deliberately absent — its temperature,
/// its cells, its alarms. Those are reached over the EC's I2C passthrough
/// rather than read from this block, cost a transfer apiece, and are asked for
/// separately under [`Capability::BatteryCondition`].
#[derive(Serialize, Deserialize, Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct BatteryInfo {
    pub state: BatteryState,
    /// What the pack holds now, in mAh.
    pub remaining_capacity: u32,
    /// What it last charged to in full, in mAh, and the denominator behind
    /// `state.percent`. It falls as the pack ages, which is what lets a pack
    /// read 100% while holding less than it once did.
    pub last_full_capacity: u32,
    /// What it was built to hold, in mAh. Taken against `last_full_capacity`
    /// it is the pack's wear.
    pub design_capacity: u32,
    /// The pack's nominal voltage in mV — what it is rated at, where
    /// `state.millivolts` is what it reads now.
    pub design_millivolts: u32,
    /// The pack's own count where it answers, and the EC's published copy
    /// otherwise — which is a floor rather than a reading, being frozen at
    /// whenever the EC last initialized the battery.
    pub cycle_count: u32,
    /// Whether a charger is attached, which `state.flow` does not settle: one
    /// too weak to cover the machine leaves the pack making up the
    /// difference, and that reads as discharging with a charger plugged in.
    pub charger_connected: bool,
    /// The EC's own low-charge alarm — its threshold, not one this app picks.
    pub critical: bool,
    pub manufacturer: String,
    pub model: String,
    pub serial: String,
    /// The cell chemistry, which the EC's memmap calls the battery type.
    pub chemistry: String,
    /// When the pack was built, as `YYYY-MM-DD`, and empty where it does not
    /// say. A date has no D-Bus type of its own, and ISO-8601 is the value's
    /// own written form rather than either end's convenience — which is why it
    /// travels as text where every other figure here travels as a number.
    pub manufactured: String,
}

/// Power button LED levels.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum PowerLedLevel {
    Auto,
    High,
    Medium,
    Low,
    UltraLow,
    /// Dark, and the only level the EC is not driving — the LED belongs to
    /// the host while it holds. Setting any other level is what gives it
    /// back, so there is no separate way to switch the LED on.
    Off,
    /// Get-only. The EC reports it after a raw percentage write and rejects
    /// it as a setting, so a caller reaches it by writing a percentage.
    Custom,
}

impl PowerLedLevel {
    /// Every level, in no order worth reading. Auto, Off and Custom sit on no
    /// scale of brightness, so any run through them is a display choice: what
    /// a level means is this crate's business, where its row sits is the
    /// front-end's.
    pub const ALL: [Self; 7] = [
        Self::Auto,
        Self::High,
        Self::Medium,
        Self::Low,
        Self::UltraLow,
        Self::Off,
        Self::Custom,
    ];

    /// Whether a setter takes this level. A predicate rather than a second
    /// list, so that a caller offering only what it can apply cannot be made
    /// wrong by where a level sits in [`Self::ALL`].
    #[must_use]
    pub const fn is_settable(self) -> bool {
        !matches!(self, Self::Custom)
    }

    /// The capability that answers for this level. Which levels a board has
    /// is a fact about its firmware, and so belongs here rather than in the
    /// process that links none of it.
    #[must_use]
    pub const fn requires(self) -> Capability {
        match self {
            Self::High | Self::Medium | Self::Low => Capability::PowerLedBrightness,
            Self::Auto | Self::UltraLow | Self::Custom => Capability::PowerLedBrightnessCustom,
            Self::Off => Capability::PowerLedOff,
        }
    }
}

/// How hard the haptic touchpad has to be pressed to register a click.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum ClickForce {
    Low,
    Medium,
    High,
}

impl ClickForce {
    /// Lightest press to firmest, which is a fact about the forces rather
    /// than a layout — so unlike [`PowerLedLevel::ALL`] a front-end can draw
    /// them in this order, and reordering here would move its rows.
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];
}

/// What a failed call says, without the D-Bus error name in front of it.
///
/// The two ends meet in this as much as in the vocabularies above, and again
/// neither can see the other's half: the daemon puts a sentence a reader can
/// act on in the error's detail — "not authorized", "no battery present" —
/// and zbus renders the pair as "{name}: {detail}", the name being machine
/// vocabulary in front of it. Taking the detail alone is what makes writing
/// that half worth the daemon's trouble. Anything but a method error renders
/// whole, having no better half to show.
#[must_use]
pub fn cause(error: &zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(_, Some(detail), _) => detail.clone(),
        other => other.to_string(),
    }
}

/// What a failed operation says, by the kind the daemon's interface answers
/// with — so a caller can tell an argument it got wrong from hardware that
/// is not there from a prompt that was declined — and the sentence for it.
///
/// The one error every implementation of the device traits below raises.
/// The direct implementation raises the kind itself; over the bus the kind
/// travels as the D-Bus error name and the sentence as its detail, and
/// [`DeviceError::from`] a `zbus::Error` puts the two back together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceError {
    InvalidArgs(String),
    /// The hardware that is there cannot do this — no EC on the board, no
    /// route to the panel. A device that is present raises it.
    NotSupported(String),
    AccessDenied(String),
    /// No such device. Only the bus implementation raises it, from the bus's
    /// unknown-interface reply: the daemon registers a device's interface
    /// only where it detected the device, so that reply is the device's
    /// absence and nothing else, and a device's `detect` reads it as such.
    Absent(String),
    Failed(String),
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArgs(m)
            | Self::NotSupported(m)
            | Self::AccessDenied(m)
            | Self::Absent(m)
            | Self::Failed(m) => f.write_str(m),
        }
    }
}

/// The kind is read off `fdo::Error`, whose derive already sorts a reply by
/// its error name; a reply outside that vocabulary keeps its sentence alone.
impl From<zbus::Error> for DeviceError {
    fn from(error: zbus::Error) -> Self {
        use zbus::fdo::Error as Fdo;
        match Fdo::from(error) {
            Fdo::InvalidArgs(m) => Self::InvalidArgs(m),
            Fdo::NotSupported(m) => Self::NotSupported(m),
            Fdo::AccessDenied(m) => Self::AccessDenied(m),
            Fdo::UnknownInterface(m) => Self::Absent(m),
            Fdo::ZBus(e) => Self::Failed(cause(&e)),
            other => Self::Failed(other.to_string()),
        }
    }
}

impl From<std::io::Error> for DeviceError {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<DeviceError> for zbus::fdo::Error {
    fn from(error: DeviceError) -> Self {
        match error {
            DeviceError::InvalidArgs(m) => Self::InvalidArgs(m),
            DeviceError::NotSupported(m) => Self::NotSupported(m),
            DeviceError::AccessDenied(m) => Self::AccessDenied(m),
            DeviceError::Absent(m) => Self::UnknownInterface(m),
            DeviceError::Failed(m) => Self::Failed(m),
        }
    }
}

pub type DeviceResult<T> = Result<T, DeviceError>;

/// What a device asks of the hardware, one trait per device and one async fn
/// per operation. Three implementations: `frameguin_hardware`'s device,
/// which touches the machine; the app's, which calls the daemon over the bus;
/// and a test's stub. A device holds only its own trait, so a stub
/// implements one and a device cannot reach past its column.
///
/// `async` for the bus, where every call is; the direct implementation never
/// pends. No `Send` bound — the app's implementor lives on one thread — and
/// a server awaiting the direct one checks its future's `Send` from the
/// concrete type.
#[allow(
    async_fn_in_trait,
    reason = "the app's implementor and its callers share one thread; the daemon's is checked as a concrete type"
)]
pub trait TouchpadControl {
    async fn haptic_intensity(&self) -> DeviceResult<u8>;
    async fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()>;
    async fn click_force(&self) -> DeviceResult<ClickForce>;
    async fn set_click_force(&self, force: ClickForce) -> DeviceResult<()>;
}

// No default_service or default_path: they would restate BUS_NAME and
// OBJECT_PATH as literals the attribute can't read a const into, leaving two
// spellings of each with nothing checking they agree. Callers name them once,
// through the proxy builder.
#[zbus::proxy(interface = "io.github.valeronm.Frameguin1")]
pub trait Frameguin {
    async fn get_charge_limit(&self) -> zbus::Result<u8>;
    /// True when the daemon wrote; false when the value was already set.
    async fn set_charge_limit(&self, percent: u8) -> zbus::Result<bool>;
    async fn get_charge_current_limit(&self) -> zbus::Result<u32>;
    async fn set_charge_current_limit(&self, milliamps: u32) -> zbus::Result<bool>;
    async fn get_battery_info(&self) -> zbus::Result<BatteryInfo>;
    async fn get_battery_condition(&self) -> zbus::Result<BatteryCondition>;
    async fn get_keyboard_backlight(&self) -> zbus::Result<u8>;
    async fn set_keyboard_backlight(&self, percent: u8) -> zbus::Result<()>;
    async fn get_capabilities(&self) -> zbus::Result<Vec<Capability>>;
    async fn get_ec_version(&self) -> zbus::Result<String>;
    async fn get_build(&self) -> zbus::Result<(String, String)>;
    async fn get_power_led_brightness(&self) -> zbus::Result<(u8, PowerLedLevel)>;
    async fn set_power_led_brightness(&self, percent: u8) -> zbus::Result<()>;
    async fn set_power_led_level(&self, level: PowerLedLevel) -> zbus::Result<()>;
    /// Whether the touch panel is on. Read from the pad the setter drives,
    /// so it answers for the hardware rather than for what this app last
    /// asked — including a value some other writer, or a boot, put there.
    async fn get_touchscreen_enabled(&self) -> zbus::Result<bool>;
    async fn set_touchscreen_enabled(&self, enabled: bool) -> zbus::Result<()>;
}

/// The haptic touchpad, on its own interface at the same path. Absent from
/// the bus on a machine without one: the daemon registers a device's
/// interface only where it detected the device, so the interfaces at
/// [`OBJECT_PATH`] are the inventory.
#[zbus::proxy(interface = "io.github.valeronm.Frameguin1.Touchpad")]
pub trait Touchpad {
    async fn get_haptic_intensity(&self) -> zbus::Result<u8>;
    async fn set_haptic_intensity(&self, percent: u8) -> zbus::Result<()>;
    async fn get_click_force(&self) -> zbus::Result<ClickForce>;
    async fn set_click_force(&self, force: ClickForce) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::{OBJECT_PATH, cause};

    fn method_error(detail: Option<&str>) -> zbus::Error {
        let reply = zbus::Message::method_call(OBJECT_PATH, "SetChargeLimit")
            .unwrap()
            .build(&())
            .unwrap();
        zbus::Error::MethodError(
            "org.freedesktop.DBus.Error.AccessDenied"
                .try_into()
                .unwrap(),
            detail.map(ToString::to_string),
            reply,
        )
    }

    /// Declining the polkit prompt is the failure every user meets, and the
    /// daemon's own half of it is already the sentence they need.
    #[test]
    fn a_method_error_reads_as_its_detail_alone() {
        assert_eq!(
            cause(&method_error(Some("not authorized"))),
            "not authorized"
        );
    }

    /// The name is all there is where a reply carries no detail, so it stays
    /// rather than leaving the sentence trailing nothing.
    #[test]
    fn an_error_without_detail_keeps_what_it_has() {
        assert!(cause(&method_error(None)).contains("AccessDenied"));
    }
}
