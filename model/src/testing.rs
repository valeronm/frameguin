use std::cell::Cell;
use std::task::{Context, Poll, Waker};

use frameguin_wire::{DeviceError, DeviceResult};

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
    let mut future = Box::pin(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("a stub never pends"),
    }
}
