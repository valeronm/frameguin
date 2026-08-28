//! The devices, one module each: what detects it, the mirror it keeps, and
//! the facets it offers — its control trait from `frameguin_wire`, its
//! [`crate::part::Part`], or both.
//!
//! A device holds only the roles it needs — a `dyn` transport and a `dyn`
//! [`crate::state::Store`] — so its logic runs against stubs in tests, and
//! nothing of the bus: authorization is the bus's business, and a caller
//! linking this crate directly has already got past it.

pub mod memory;
pub mod touchpad;
