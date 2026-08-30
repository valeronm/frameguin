//! One trait per device, one async fn per operation: what a device asks of
//! the hardware, and what three implementations answer — `frameguin_hardware`'s
//! device, which touches the machine; the app's, which calls the daemon over
//! the bus; and a test's stub. A device holds only its own trait, so a stub
//! implements one and a device cannot reach past its column.
//!
//! `async` for the bus, where every call is; the direct implementation never
//! pends. No `Send` bound — the app's implementor lives on one thread — and
//! a server awaiting the direct one checks its future's `Send` from the
//! concrete type.

use crate::error::DeviceResult;
use crate::vocabulary::{BatteryCondition, BatteryFeature, BatteryInfo, ClickForce, PowerLedLevel};

pub trait TouchpadControl {
    async fn haptic_intensity(&self) -> DeviceResult<u8>;
    async fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()>;
    async fn click_force(&self) -> DeviceResult<ClickForce>;
    async fn set_click_force(&self, force: ClickForce) -> DeviceResult<()>;
}

/// The touch panel, named for the panel rather than for the way it is
/// reached, which differs by machine and is the device's business alone:
/// this end asks whether touch is on, never how it is switched.
pub trait TouchscreenControl {
    /// Whether the panel is reporting: the hardware's own account where the
    /// route keeps one, the device's record of its last write where not — so
    /// a value some other writer, or a boot, put there arrives the same way.
    async fn enabled(&self) -> DeviceResult<bool>;
    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()>;
}

/// The power button LED: a level, and behind every level a percentage.
pub trait PowerLedControl {
    /// The percentage and the level it belongs to. The level can be `Custom`,
    /// which the EC reports after any raw percentage write, or `Off`, which
    /// the EC cannot report at all — it is the host holding the LED, and
    /// the percentage beside it is what the EC will light it at when the
    /// host lets go.
    async fn brightness(&self) -> DeviceResult<(u8, PowerLedLevel)>;
    /// Every level this board has, `Custom` included where a percentage
    /// can be written; fixed for the device's run.
    async fn levels(&self) -> DeviceResult<Vec<PowerLedLevel>>;
    async fn set_level(&self, level: PowerLedLevel) -> DeviceResult<()>;
    async fn set_brightness(&self, percent: u8) -> DeviceResult<()>;
}

/// The battery: the pack the EC's block answers for, and the charger that
/// shapes what goes into it.
pub trait BatteryControl {
    /// One walk of the EC's block. The charge is the one value here that
    /// changes without anyone setting it, so a caller showing it re-reads.
    async fn info(&self) -> DeviceResult<BatteryInfo>;
    /// What the pack says about itself, offered only under
    /// [`BatteryFeature::Condition`]: a transfer per cell plus two, to a
    /// device the EC is also driving.
    async fn condition(&self) -> DeviceResult<BatteryCondition>;
    /// What this battery offers past its block; fixed for the device's run.
    async fn features(&self) -> DeviceResult<Vec<BatteryFeature>>;
    async fn charge_limit(&self) -> DeviceResult<u8>;
    /// True when the hardware was written; false when the value was found
    /// already in place and left alone. The one place the bus's skip shows
    /// in a contract: a caller announces a change and not a request for
    /// what already held, and only the bus, which skips to spare the
    /// authorization prompt, can tell the two apart — the device writes
    /// whatever it is handed and answers true.
    async fn set_charge_limit(&self, percent: u8) -> DeviceResult<bool>;
    /// The cap in mA, or [`NO_CHARGE_CURRENT_LIMIT`] when nothing caps it.
    /// The EC cannot be asked what it holds, so this is what was last
    /// written, and reports no limit once the EC has restarted and dropped
    /// the value — or will not say whether it has.
    ///
    /// [`NO_CHARGE_CURRENT_LIMIT`]: crate::NO_CHARGE_CURRENT_LIMIT
    async fn charge_current_limit(&self) -> DeviceResult<u32>;
    /// Caps how fast the battery charges; [`NO_CHARGE_CURRENT_LIMIT`] lifts
    /// the cap. Zero is refused: the EC clamps its requested current against
    /// this value, so zero stops charging altogether. Returns whether the
    /// hardware was written, as `set_charge_limit` does.
    ///
    /// [`NO_CHARGE_CURRENT_LIMIT`]: crate::NO_CHARGE_CURRENT_LIMIT
    async fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<bool>;
}
