//! The vocabulary of `io.github.valeronm.Frameguin1`, declared once: where
//! the daemon's `#[interface]` impl and the app's calls — two independent
//! restatements of one interface that meet only at runtime — are made to
//! agree at compile time instead. One flat namespace, whichever file an
//! item sits in.
//!
//! Beside the interface sit the strings both binaries must spell alike —
//! [`VENDOR`], the board names.

#![allow(
    async_fn_in_trait,
    reason = "every trait here is a control: the app's implementor and its callers share one thread, and the daemon's is checked as a concrete type"
)]

mod control;
mod error;
mod proxies;
mod vocabulary;

pub use control::*;
pub use error::*;
pub use proxies::*;
pub use vocabulary::*;
