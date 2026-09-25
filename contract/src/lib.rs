//! What every implementation of a control and every caller of one agree on,
//! declared once: the control traits, the values they carry with the
//! encoding those values cross the bus in, the one error they raise, and the
//! strings both binaries must spell alike — [`VENDOR`] — along with
//! [`Series`], which is derived from one rather than carried. One flat
//! namespace, whichever file an item sits in.

#![allow(
    async_fn_in_trait,
    reason = "every trait here is a control: the app's implementor and its callers share one thread, and the daemon's is checked as a concrete type"
)]

mod control;
mod error;
mod vocabulary;

pub use control::*;
pub use error::*;
pub use vocabulary::*;
