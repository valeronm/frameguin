//! Direct access to the hardware: the transports, the roles a device needs
//! of them, and the devices over those roles.
//!
//! A device is a thing detection finds on the machine. What it offers is
//! said by the traits it implements: a control trait from `frameguin_wire`
//! where it can be read and set, [`part::Part`] where it is something a
//! person bought. This crate is the one implementation of the control
//! traits that touches the machine. The daemon serves it over the bus; the
//! app implements the same traits by calling the daemon; a test implements
//! them with a stub. A process linking this crate has hardware access,
//! which is why only the daemon does.
//!
//! The transport modules divide by how the machine is reached, so the
//! filename answers which way: [`ec`] the EC, [`led`] the kernel's LED
//! class, [`touchpad`] the pad's own HID transport, [`panel`] the touch
//! panel's, [`gpio`] a pad on the processor through the GPIO character
//! device, [`dmi`] the firmware's SMBIOS table. [`touchscreen`] settles
//! which of two routes a machine has, and is the role over either.
//! [`lifetime`] is what holds a mirrored
//! value, [`state`] the store for what cannot be read back, [`part`] what a
//! device is as a part of the machine, and [`device`] the devices
//! themselves.

#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    reason = "a workspace-private crate with one caller; its signatures are the contract"
)]

pub mod device;
pub mod dmi;
pub mod ec;
pub mod gpio;
pub mod led;
pub mod lifetime;
pub mod panel;
pub mod part;
pub mod state;
pub mod touchpad;
pub mod touchscreen;

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use frameguin_wire::{DeviceError, DeviceResult};

    use crate::ec::EcClock;
    use crate::lifetime::EcStamp;

    /// An EC clock answering one way about every stamp, or not at all.
    pub(crate) struct Clock {
        pub(crate) same_boot: Mutex<Option<bool>>,
    }

    impl Clock {
        pub(crate) fn new(same_boot: Option<bool>) -> Arc<Self> {
            Arc::new(Self {
                same_boot: Mutex::new(same_boot),
            })
        }

        fn answer(&self) -> DeviceResult<bool> {
            self.same_boot
                .lock()
                .unwrap()
                .ok_or_else(|| DeviceError::Failed("no EC".into()))
        }
    }

    impl EcClock for Clock {
        fn stamp(&self) -> DeviceResult<EcStamp> {
            self.answer().map(|_| EcStamp::taken(500, 1_000_000))
        }

        fn same_boot_as(&self, _stamp: EcStamp) -> bool {
            *self.same_boot.lock().unwrap() == Some(true)
        }
    }

    /// Polls once: the direct implementation never pends.
    pub(crate) fn ready<T>(future: impl Future<Output = T>) -> T {
        let mut future = Box::pin(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("the direct implementation never pends"),
        }
    }
}
