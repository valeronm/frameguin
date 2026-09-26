//! The app's side of the machine: its controls, over the control traits
//! `frameguin_contract` declares, and the curated facts about the machine —
//! it holds no user-facing string.
//!
//! A control here is the client of one: the read that fills a front-end's
//! rows and the commands that move the hardware. It holds one control trait
//! and nothing else, so a control runs against a stub in tests.

#![allow(
    clippy::missing_errors_doc,
    reason = "every Result here fails one way: the daemon's own sentence, carried whole in DeviceError"
)]

pub mod control;
pub mod port;
pub mod reading;

#[cfg(any(test, feature = "testing"))]
pub mod fixtures;
#[cfg(test)]
pub(crate) mod testing;
