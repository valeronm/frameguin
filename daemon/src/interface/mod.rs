//! One D-Bus interface per device's control, each a thin adapter on
//! [`crate::served::Served`] over `frameguin_hardware`'s implementation of
//! that control trait.
//!
//! What an adapter adds is the bus's business alone — the idle clock, and
//! the order validate → skip → authorize → write, with the polkit prompt in
//! the place that order puts it. The operation itself, its argument check
//! included, is the device's, so a caller reaching the hardware crate
//! directly gets the same refusals without this layer.

pub(crate) mod touchpad;
pub(crate) mod touchscreen;
