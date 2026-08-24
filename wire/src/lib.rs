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
    /// A pack the EC's memmap block answers for. The only reading in the
    /// interface that moves on its own, and the only one no setter pairs
    /// with.
    BatteryState,
    ChargeLimit,
    ChargeCurrentLimit,
    KeyboardBacklight,
    FpBrightness,
    /// V1 of the EC's fingerprint command: the raw percentage write, and with
    /// it the ultra-low and auto levels (framework-system issue #211).
    FpBrightnessCustom,
    /// Darkening the LED, which the EC's command cannot express: it takes
    /// 1-100 and reserves 0, the LED being the power indicator too. Off is
    /// reached by taking the LED off the EC's own policy through the kernel's
    /// LED class, so this capability answers for that interface rather than
    /// for a command version.
    FpOff,
    /// One name for both touchpad controls — same device, same firmware
    /// feature set, so nothing can support one and not the other.
    HapticTouchpad,
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

/// Fingerprint LED levels.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum FpLevel {
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

impl FpLevel {
    /// The automatic mode, then brightest to dimmest with dark past the
    /// dimmest, then the get-only one.
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
    /// wrong by the order the variants happen to be listed in.
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
            Self::High | Self::Medium | Self::Low => Capability::FpBrightness,
            Self::Auto | Self::UltraLow | Self::Custom => Capability::FpBrightnessCustom,
            Self::Off => Capability::FpOff,
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
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];
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
    async fn get_battery_design_capacity(&self) -> zbus::Result<u32>;
    async fn get_battery_state(&self) -> zbus::Result<BatteryState>;
    async fn get_keyboard_backlight(&self) -> zbus::Result<u8>;
    async fn set_keyboard_backlight(&self, percent: u8) -> zbus::Result<()>;
    async fn get_capabilities(&self) -> zbus::Result<Vec<Capability>>;
    async fn get_ec_version(&self) -> zbus::Result<String>;
    async fn get_build(&self) -> zbus::Result<(String, String)>;
    async fn get_fingerprint_brightness(&self) -> zbus::Result<(u8, FpLevel)>;
    async fn set_fingerprint_brightness(&self, percent: u8) -> zbus::Result<()>;
    async fn set_fingerprint_level(&self, level: FpLevel) -> zbus::Result<()>;
    async fn get_haptic_intensity(&self) -> zbus::Result<u8>;
    async fn set_haptic_intensity(&self, percent: u8) -> zbus::Result<()>;
    async fn get_touchpad_click_force(&self) -> zbus::Result<ClickForce>;
    async fn set_touchpad_click_force(&self, force: ClickForce) -> zbus::Result<()>;
}
