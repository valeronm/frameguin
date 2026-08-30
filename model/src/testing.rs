use std::cell::Cell;
use std::task::{Context, Poll, Waker};

use frameguin_wire::{BatteryInfo, BatteryState, ChargeFlow, DeviceError, DeviceResult};

/// A 4640 mAh pack, the Laptop 13's.
pub(crate) const CAPACITY: u32 = 4640;

/// Mid-charge on the same pack's four cells.
pub(crate) const MILLIVOLTS: u32 = 15_400;

/// What that pack is rated at, which is what its energy is measured against
/// however charged it happens to be.
pub(crate) const NOMINAL_MILLIVOLTS: u32 = 15_640;

pub(crate) fn state(flow: ChargeFlow, milliamps: u32) -> BatteryState {
    BatteryState {
        percent: 62,
        flow,
        milliamps,
        millivolts: MILLIVOLTS,
    }
}

/// The pack's block, part way through a charge.
pub(crate) fn block() -> BatteryInfo {
    BatteryInfo {
        state: state(ChargeFlow::Charging, 2320),
        remaining_capacity: 2843,
        last_full_capacity: 4176,
        design_capacity: CAPACITY,
        design_millivolts: NOMINAL_MILLIVOLTS,
        cycle_count: 40,
        charger_connected: true,
        critical: false,
        manufacturer: "NVT".into(),
        model: "FRANGWA".into(),
        serial: String::new(),
        chemistry: "LION".into(),
        manufactured: String::new(),
    }
}

/// What a stub does besides answer: refuses every write once told to, and
/// answers every read with `failing` where one is set.
#[derive(Default)]
pub(crate) struct Fault {
    refusing: Cell<bool>,
    failing: Option<DeviceError>,
}

impl Fault {
    pub(crate) fn failing(error: DeviceError) -> Self {
        Self {
            failing: Some(error),
            ..Self::default()
        }
    }

    pub(crate) fn refuse(&self) {
        self.refusing.set(true);
    }

    pub(crate) fn write(&self) -> DeviceResult<()> {
        if self.refusing.get() {
            Err(DeviceError::AccessDenied("not authorized".into()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn read<T>(&self, value: T) -> DeviceResult<T> {
        self.failing.clone().map_or(Ok(value), Err)
    }
}

/// Polls once: a stub answers on the spot, so a future here never pends.
pub(crate) fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("a stub never pends"),
    }
}
