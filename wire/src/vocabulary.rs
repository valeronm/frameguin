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
//! An enum whose variants carry nothing serializes as `s`, so the wire format
//! is the plain string the variant is named after. zvariant encodes an enum's
//! fields only where every variant carries the same ones.

use serde::{Deserialize, Serialize};
use zbus::zvariant::{Type, Value};

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
pub const BOARD_LAPTOP12_CORE_3: &str = "Laptop 12 (Intel Core Series 3)";
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
/// passthrough, the two limits the charger takes, and the extender's state.
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
    Extender,
}

/// How far the EC's battery extender has lowered the window it holds a
/// charged pack in.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum ExtenderStage {
    /// Holding nothing of its own, whether counting down or switched off.
    Inactive,
    First,
    Second,
}

/// Framework's battery extender as the EC reports it.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct ExtenderState {
    pub enabled: bool,
    pub stage: ExtenderStage,
    /// Days from the EC's start or the last reset to the first stage.
    pub trigger_days: u16,
    /// Seconds left until the first stage, and 0 where no countdown runs —
    /// the first stage reached, or the extender switched off.
    pub first_stage_seconds: u32,
    /// Minutes continuously off the charger that return the extender to
    /// [`ExtenderStage::Inactive`] and restart its countdown.
    pub reset_minutes: u16,
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

/// What the EC's memmap battery block says about the pack's charge and wear,
/// for a reader looking at the pack itself rather than at the controls that
/// shape it; what names and rates the pack is fixed for the run, and is its
/// part's [`Identity`] instead.
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

    /// The level's name, spelled as it travels on the wire.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::UltraLow => "ultra-low",
            Self::Off => "off",
            Self::Custom => "custom",
        }
    }

    /// The inverse of [`Self::name`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.name() == name)
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

#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct ChassisState {
    pub open: bool,
    /// Times the switch opened while the EC was running.
    pub opened: u8,
    /// Times the EC started with the chassis already open.
    pub found_open: u8,
}

/// What a chassis offers past its switch, each a separate question of the
/// hardware.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum ChassisFeature {
    Deck,
}

/// The EC's power state for the input deck.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum DeckState {
    /// The host is off.
    Off,
    /// Powered host, no deck detected.
    Disconnected,
    TurningOn,
    On,
    ForceOff,
    ForceOn,
    /// Powered with the host, no presence check.
    NoDetection,
}

/// True where a switch leaves its device connected.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct PrivacyState {
    pub camera: bool,
    pub microphone: bool,
}

/// The rate a USB link negotiated, as the kernel's `speed` attribute names
/// it. `Unknown` is the arm for a `speed` string this vocabulary does not
/// name.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "s")]
#[serde(rename_all = "kebab-case")]
pub enum UsbSpeed {
    Low,
    Full,
    High,
    Super,
    SuperPlus,
    SuperPlus2x2,
    Unknown,
}

/// A USB device plugged straight into a root port, in its own words.
///
/// `controller` and `root_port` are what places it: the USB bus number is
/// handed out in enumeration order and names no controller.
#[derive(Serialize, Deserialize, Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct Attached {
    /// The host controller's PCI address, `0000:00:14.0`.
    pub controller: String,
    pub root_port: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    /// Empty where the device announces none.
    pub manufacturer: String,
    /// Empty where the device announces none.
    pub product: String,
    pub speed: UsbSpeed,
    /// The running firmware of a card whose version can be read without
    /// writing to it, and empty for every other device.
    pub firmware: String,
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
    Storage,
    Display,
    Touchpad,
}

/// Something else a part announced about itself, each fact in its own unit.
///
/// A detail crosses the bus as the fact's name and its value.
#[derive(Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "(sv)")]
pub enum Detail {
    /// In bytes; a module is sold in binary units.
    MemoryCapacity(u64),
    /// In bytes; a drive is sold in decimal units.
    StorageCapacity(u64),
    /// What a pack was built to hold, in mAh, beside its nominal voltage in
    /// mV — a pack is sold by the energy the two make.
    DesignCapacity {
        milliamp_hours: u32,
        millivolts: u32,
    },
    /// What a pack is rated at, in mV.
    NominalVoltage(u32),
    /// As `YYYY-MM-DD`. A date has no D-Bus type of its own, and ISO-8601 is
    /// the value's own written form rather than either end's convenience.
    ManufactureDate(String),
    /// The active pixels of the timing the panel prefers.
    Resolution {
        across: u16,
        down: u16,
    },
    /// In millimetres.
    PanelSize {
        across: u16,
        down: u16,
    },
    /// Bits per colour.
    ColourDepth(u8),
    /// The vertical rates a panel accepts, in whole Hz — both ends the same
    /// on a panel of one rate.
    RefreshRate {
        slowest: u16,
        fastest: u16,
    },
    ManufactureYear(u16),
    ModelYear(u16),
    /// As the SMBIOS specification names the byte, or the byte in hex where
    /// it names none.
    MemoryType(String),
    /// As [`Detail::MemoryType`].
    FormFactor(String),
    /// In MT/s.
    Speed(u32),
    /// In MT/s.
    ConfiguredSpeed(u32),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Fact {
    MemoryCapacity,
    StorageCapacity,
    DesignCapacity,
    NominalVoltage,
    ManufactureDate,
    Resolution,
    PanelSize,
    ColourDepth,
    RefreshRate,
    ManufactureYear,
    ModelYear,
    MemoryType,
    FormFactor,
    Speed,
    ConfiguredSpeed,
}

impl Serialize for Detail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (fact, value) = match self {
            Self::MemoryCapacity(bytes) => (Fact::MemoryCapacity, Value::from(*bytes)),
            Self::StorageCapacity(bytes) => (Fact::StorageCapacity, Value::from(*bytes)),
            Self::DesignCapacity {
                milliamp_hours,
                millivolts,
            } => (
                Fact::DesignCapacity,
                Value::from((*milliamp_hours, *millivolts)),
            ),
            Self::NominalVoltage(millivolts) => (Fact::NominalVoltage, Value::from(*millivolts)),
            Self::ManufactureDate(date) => (Fact::ManufactureDate, Value::from(date.as_str())),
            Self::Resolution { across, down } => (Fact::Resolution, Value::from((*across, *down))),
            Self::PanelSize { across, down } => (Fact::PanelSize, Value::from((*across, *down))),
            Self::ColourDepth(bits) => (Fact::ColourDepth, Value::from(*bits)),
            Self::RefreshRate { slowest, fastest } => {
                (Fact::RefreshRate, Value::from((*slowest, *fastest)))
            }
            Self::ManufactureYear(year) => (Fact::ManufactureYear, Value::from(*year)),
            Self::ModelYear(year) => (Fact::ModelYear, Value::from(*year)),
            Self::MemoryType(name) => (Fact::MemoryType, Value::from(name.as_str())),
            Self::FormFactor(name) => (Fact::FormFactor, Value::from(name.as_str())),
            Self::Speed(rate) => (Fact::Speed, Value::from(*rate)),
            Self::ConfiguredSpeed(rate) => (Fact::ConfiguredSpeed, Value::from(*rate)),
        };
        (fact, value).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Detail {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (fact, value) = <(Fact, Value<'de>)>::deserialize(deserializer)?;
        let detail = match fact {
            Fact::MemoryCapacity => u64::try_from(value).map(Self::MemoryCapacity),
            Fact::StorageCapacity => u64::try_from(value).map(Self::StorageCapacity),
            Fact::DesignCapacity => {
                <(u32, u32)>::try_from(value).map(|(milliamp_hours, millivolts)| {
                    Self::DesignCapacity {
                        milliamp_hours,
                        millivolts,
                    }
                })
            }
            Fact::NominalVoltage => u32::try_from(value).map(Self::NominalVoltage),
            Fact::ManufactureDate => String::try_from(value).map(Self::ManufactureDate),
            Fact::Resolution => <(u16, u16)>::try_from(value)
                .map(|(across, down)| Self::Resolution { across, down }),
            Fact::PanelSize => {
                <(u16, u16)>::try_from(value).map(|(across, down)| Self::PanelSize { across, down })
            }
            Fact::ColourDepth => u8::try_from(value).map(Self::ColourDepth),
            Fact::RefreshRate => <(u16, u16)>::try_from(value)
                .map(|(slowest, fastest)| Self::RefreshRate { slowest, fastest }),
            Fact::ManufactureYear => u16::try_from(value).map(Self::ManufactureYear),
            Fact::ModelYear => u16::try_from(value).map(Self::ModelYear),
            Fact::MemoryType => String::try_from(value).map(Self::MemoryType),
            Fact::FormFactor => String::try_from(value).map(Self::FormFactor),
            Fact::Speed => u32::try_from(value).map(Self::Speed),
            Fact::ConfiguredSpeed => u32::try_from(value).map(Self::ConfiguredSpeed),
        };
        detail.map_err(serde::de::Error::custom)
    }
}

/// What carries a firmware.
///
/// A kind crosses the bus as its name and the controller number, zero for a
/// kind that has none.
#[derive(Type, Clone, Copy, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant", signature = "(sy)")]
pub enum FirmwareKind {
    Bios,
    Ec,
    /// A USB-C power delivery controller, by the EC's controller number.
    PowerDelivery(u8),
    Drive,
    TouchController,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Carrier {
    Bios,
    Ec,
    PowerDelivery,
    Drive,
    TouchController,
}

impl Serialize for FirmwareKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match *self {
            Self::Bios => (Carrier::Bios, 0u8),
            Self::Ec => (Carrier::Ec, 0),
            Self::PowerDelivery(controller) => (Carrier::PowerDelivery, controller),
            Self::Drive => (Carrier::Drive, 0),
            Self::TouchController => (Carrier::TouchController, 0),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FirmwareKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (carrier, controller) = <(Carrier, u8)>::deserialize(deserializer)?;
        Ok(match carrier {
            Carrier::Bios => Self::Bios,
            Carrier::Ec => Self::Ec,
            Carrier::PowerDelivery => Self::PowerDelivery(controller),
            Carrier::Drive => Self::Drive,
            Carrier::TouchController => Self::TouchController,
        })
    }
}

#[derive(Serialize, Deserialize, Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct Firmware {
    pub kind: FirmwareKind,
    /// As the vendor spells it.
    pub version: String,
    /// When the firmware was built: an ISO date, with the time where the
    /// source carries one, and whatever it stamped instead where that is not
    /// a date at all. Empty where it announced a version and nothing more.
    pub built: String,
    /// Whoever built it, and empty where the firmware announced no builder.
    pub builder: String,
}

impl Firmware {
    #[must_use]
    pub fn new(kind: FirmwareKind, version: &str) -> Self {
        Self {
            kind,
            version: version.to_owned(),
            built: String::new(),
            builder: String::new(),
        }
    }
}

/// What detection found of a part: the hardware's own words but for the
/// vendor name a registry supplies, and never the name a person buys it
/// under, which is `model`'s catalogue to say.
#[derive(Serialize, Deserialize, Type, Clone, PartialEq, Eq, Debug)]
#[zvariant(crate = "zbus::zvariant")]
pub struct Identity {
    pub kind: PartKind,
    pub vendor: String,
    /// What a registry calls the vendor, and empty where nothing names it.
    pub vendor_name: String,
    pub model: String,
    /// Empty where the model is already the part's own number.
    pub part_number: String,
    /// Empty where the part announces none, as some descriptors do.
    pub serial: String,
    /// The identifier the part announces itself by, prefixed with the space
    /// it is drawn from — `hid:093a:1343`, `dmi-slot:LPCAMM2_0`,
    /// `dmi-board:FRANMJCP07`.
    pub id: String,
    /// Every firmware the part would report — a version is never worth a
    /// failed detection, so one it would not say is left out.
    pub firmware: Vec<Firmware>,
    /// What the part announced beyond its identity, empty for one that
    /// announced nothing.
    pub details: Vec<Detail>,
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

#[cfg(test)]
mod tests {
    use zbus::zvariant::serialized::Context;
    use zbus::zvariant::{LE, to_bytes};

    use super::{Attached, Detail, FirmwareKind, UsbSpeed};

    #[test]
    fn every_detail_crosses_the_bus_as_itself() {
        let details = vec![
            Detail::MemoryCapacity(32 << 30),
            Detail::StorageCapacity(1_024_209_543_168),
            Detail::DesignCapacity {
                milliamp_hours: 4_800,
                millivolts: 15_400,
            },
            Detail::NominalVoltage(15_400),
            Detail::ManufactureDate("2025-03-14".to_owned()),
            Detail::Resolution {
                across: 2880,
                down: 1920,
            },
            Detail::PanelSize {
                across: 285,
                down: 190,
            },
            Detail::ColourDepth(10),
            Detail::RefreshRate {
                slowest: 30,
                fastest: 120,
            },
            Detail::ManufactureYear(2025),
            Detail::ModelYear(2024),
            Detail::MemoryType("LPDDR5".to_owned()),
            Detail::FormFactor("CAMM".to_owned()),
            Detail::Speed(8533),
            Detail::ConfiguredSpeed(7467),
        ];
        let encoded = to_bytes(Context::new_dbus(LE, 0), &details).unwrap();
        let decoded: Vec<Detail> = encoded.deserialize().unwrap().0;
        assert_eq!(decoded, details);
    }

    #[test]
    fn every_firmware_kind_crosses_the_bus_as_itself() {
        let kinds = vec![
            FirmwareKind::Bios,
            FirmwareKind::Ec,
            FirmwareKind::PowerDelivery(2),
            FirmwareKind::Drive,
            FirmwareKind::TouchController,
        ];
        let encoded = to_bytes(Context::new_dbus(LE, 0), &kinds).unwrap();
        let decoded: Vec<FirmwareKind> = encoded.deserialize().unwrap().0;
        assert_eq!(decoded, kinds);
    }

    #[test]
    fn an_attached_device_crosses_the_bus_as_itself() {
        let attached = vec![Attached {
            controller: "0000:00:14.0".to_owned(),
            root_port: 5,
            vendor_id: 0x32ac,
            product_id: 0x0002,
            manufacturer: "Framework".to_owned(),
            product: "HDMI Expansion Card".to_owned(),
            speed: UsbSpeed::Full,
            firmware: String::new(),
        }];
        let ctxt = Context::new_dbus(LE, 0);
        let bytes = to_bytes(ctxt, &attached).unwrap();
        let (back, _): (Vec<Attached>, _) = bytes.deserialize().unwrap();
        assert_eq!(back, attached);
    }
}
