//! The app's side of the machine: its controls, over the control traits
//! `frameguin_wire` declares.
//!
//! A control here is the client of one: the read that fills a front-end's
//! rows, the commands that move the hardware, and the presets and words a
//! front-end draws it with. It holds one control trait and nothing else, so
//! the window and the tray cannot disagree about what a value is called or
//! what a command sends, and a control runs against a stub in tests.

#![allow(
    clippy::missing_errors_doc,
    reason = "every Result here fails one way: the daemon's own sentence, carried whole in DeviceError"
)]

pub mod control;

#[cfg(test)]
pub(crate) mod testing {
    use std::task::{Context, Poll, Waker};

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
}
