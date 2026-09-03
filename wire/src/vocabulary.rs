//! The names, numbers and records both ends spell.
//!
//! A name or a signature the two ends disagree about comes back as an error
//! reply, and a *string* they disagree about — a feature, a level, a click
//! force — comes back as a value that is well-formed and meaningless. Naming
//! those strings as types is what moves that second class of drift to
//! compile time. A number both ends must simply agree on belongs here for
//! the same reason, and fails more quietly still: the receiver accepts it
//! and acts on it.
//!
//! Every enum here serializes as `s`, so the wire format is the plain string
//! the variant is named after.

use std::fmt;

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

// A board's DMI `product_name`, in the firmware's own spelling, matched
// whole and never parsed. They are the strings `framework_lib` matches to
// identify a platform, re-spelled because the type it answers with is
// private to that crate.
pub const BOARD_LAPTOP13_11TH_GEN: &str = "Laptop";
pub const BOARD_LAPTOP13_12TH_GEN: &str = "Laptop (12th Gen Intel Core)";
pub const BOARD_LAPTOP13_13TH_GEN: &str = "Laptop (13th Gen Intel Core)";
pub const BOARD_LAPTOP13_ULTRA_1: &str = "Laptop 13 (Intel Core Ultra Series 1)";
pub const BOARD_LAPTOP13_AMD_7040: &str = "Laptop 13 (AMD Ryzen 7040 Series)";
// Some 7040 firmware ships the series without its space.
pub const BOARD_LAPTOP13_AMD_7040_UNSPACED: &str = "Laptop 13 (AMD Ryzen 7040Series)";
pub const BOARD_LAPTOP13_AMD_AI_300: &str = "Laptop 13 (AMD Ryzen AI 300 Series)";
pub const BOARD_LAPTOP13_PRO_ULTRA_3: &str = "Laptop 13 Pro (Intel Core Ultra Series 3)";
pub const BOARD_LAPTOP12_13TH_GEN: &str = "Laptop 12 (13th Gen Intel Core)";
pub const BOARD_LAPTOP16_AMD_7040: &str = "Laptop 16 (AMD Ryzen 7040 Series)";
pub const BOARD_LAPTOP16_AMD_AI_300: &str = "Laptop 16 (AMD Ryzen AI 300 Series)";
pub const BOARD_DESKTOP_AMD_AI_MAX_300: &str = "Desktop (AMD Ryzen AI Max 300 Series)";

/// Charge as fast as the battery asks. The EC clamps every requested charge
/// current against its limit, so the largest value is the one that imposes
/// none; 0 at the other end would mean never charge, which no setter accepts.
pub const NO_CHARGE_CURRENT_LIMIT: u32 = u32::MAX;

/// The lowest charge limit `SetChargeLimit` accepts, and so a slider's
/// floor. Spelled here because both ends must agree on it and neither can
/// see the other's copy.
pub const MIN_CHARGE_LIMIT: u8 = 20;

/// Every intensity `SetHapticIntensity` accepts. The touchpad firmware
/// implements five steps rather than the 0-100 its HID descriptor advertises,
/// and this is the one control whose legal arguments the app cannot look up
/// for itself — the crate that knows them is the one it must not link.
pub const HAPTIC_INTENSITY_LEVELS: [u8; 5] = [0, 25, 50, 75, 100];

/// What a battery offers past the block every pack answers with, each a
/// separate question of the hardware: the pack's own report over the EC's
/// passthrough, and the two limits the charger takes.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum BatteryFeature {
    /// What the pack says about itself past the EC's summary of it — its
    /// temperature, its cell voltages, and the alarms it is raising. One name
    /// for all three: they are the same device over the same transport,
    /// reached by the same passthrough, so a pack answering for one answers
    /// for the others.
    Condition,
    ChargeLimit,
    ChargeCurrentLimit,
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
/// separately under [`BatteryFeature::Condition`].
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
}

/// What is on the other end of a USB-C port, as the controller's Type-C
/// state machine has it. `Nothing` is an empty port, and the only value that
/// says so — a port with nothing in it still reports roles, which mean
/// nothing until something is attached.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum PortPartner {
    Nothing,
    Sink,
    Source,
    Debug,
    Audio,
    PoweredAccessory,
    Unsupported,
    Invalid,
}

/// Which end supplies the power. The machine reads `Sink` on a port it is
/// charging from and `Source` on one feeding a peripheral.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum PowerRole {
    Sink,
    Source,
    Unknown,
}

/// Which end drives the data link — upstream-facing is the machine being
/// the peripheral.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum DataRole {
    UpstreamFacing,
    DownstreamFacing,
    Disconnected,
    Unknown,
}

/// Which configuration channel the cable settled on, which is the plug's
/// orientation.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum CcPolarity {
    Cc1,
    Cc2,
    Cc1Debug,
    Cc2Debug,
    Unknown,
}

/// Extended power range, which is what carries a supply past 100 W. One
/// value rather than a supported flag and an active one, active implying
/// supported and the pair having no other order.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum Epr {
    Unsupported,
    Supported,
    Active,
}

/// One USB-C port, as the EC's copy of its controller's state has it.
///
/// Every field is the EC's cache rather than the port itself, which matters
/// in one case: a controller whose ports have been disabled stops updating
/// it, and the entry then stands at whatever it last saw. Nothing in this
/// app disables one — see `docs/hardware.md`.
#[derive(Serialize, Deserialize, Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is an independent thing the EC reports about a port; the one pair with an impossible combination is Epr"
)]
pub struct PortState {
    /// The EC's own port number: which controller, and which of its two
    /// connectors. It says nothing about where the socket is on the
    /// chassis, that mapping being per board — `model::port` is what puts a
    /// position to it, and only for a board it was measured on.
    pub index: u8,
    pub partner: PortPartner,
    /// Whether a power delivery contract stands. False on a Type-C
    /// connection that negotiated none, which still carries power.
    pub contract: bool,
    pub power_role: PowerRole,
    pub data_role: DataRole,
    /// What was negotiated, in mV and mA; both zero where nothing was.
    pub millivolts: u16,
    pub milliamps: u16,
    /// Whether this is the port the machine is drawing its power through.
    /// At most one port answers true, the EC picking among those offering.
    pub charging: bool,
    /// Whether the port is carrying `DisplayPort`.
    pub video: bool,
    pub vconn: bool,
    pub cc: CcPolarity,
    pub epr: Epr,
}

/// What kind of part a device is, named for the thing a person would buy.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum PartKind {
    Mainboard,
    Battery,
    Memory,
    Touchpad,
    Touchscreen,
}

/// One firmware a part runs, named for what carries it as the part's user
/// would: `BIOS` and `EC` on the mainboard, `Controller` on a touch panel.
#[derive(Serialize, Deserialize, Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct Firmware {
    pub name: String,
    /// As the vendor spells it.
    pub version: String,
}

impl Firmware {
    #[must_use]
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_owned(),
            version: version.to_owned(),
        }
    }
}

/// What detection saw of a part, kept as it was announced: the words are the
/// hardware's own, and the name a person buys it under is `model`'s
/// catalogue to say.
#[derive(Serialize, Deserialize, Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct Identity {
    pub kind: PartKind,
    pub vendor: String,
    pub model: String,
    /// Empty where the part announces none, as some descriptors do.
    pub serial: String,
    /// The identifier the part announces itself by, prefixed with the space
    /// it is drawn from — `hid:093a:1343`, `dmi-slot:LPCAMM2_0`,
    /// `dmi-board:FRANMJCP07`.
    pub id: String,
    /// Every firmware the part would report — a version is never worth a
    /// failed detection, so one it would not say is left out.
    pub firmware: Vec<Firmware>,
}

/// One line per part, as the daemon's journal and the app's debug report
/// both print it — `Mainboard dmi-board:… "vendor" "model" firmware BIOS …`
/// — so a bug report and the log it is read against spell a part the same.
impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let firmware = if self.firmware.is_empty() {
            "unknown".to_owned()
        } else {
            self.firmware
                .iter()
                .map(|f| format!("{} {}", f.name, f.version))
                .collect::<Vec<_>>()
                .join(", ")
        };
        write!(
            f,
            "{:?} {} \"{}\" \"{}\" firmware {firmware}",
            self.kind, self.id, self.vendor, self.model
        )
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
