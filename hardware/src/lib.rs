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
//! filename answers which way: `ec` the EC, `led` the kernel's LED class,
//! `touchpad` the pad's own HID transport, `panel` the touch panel's,
//! `gpio` a pad on the processor through the GPIO character device, `dmi`
//! the firmware's SMBIOS table, `drm` the kernel's DRM class, `nvme` the
//! kernel's `NVMe` class. `touchscreen` settles which of two routes a
//! machine has, and is the role over either. `sbs` is the pack's own
//! registers and what their words mean, `pd` what the EC's cached PD
//! controller version means, `edid` what a panel's own block says it is,
//! `state` the store for what cannot be read back and what was asked for,
//! `lifetime` what holds a mirrored value and how to tell it still does,
//! [`mirror`] the mirror a device reads and writes such a value through,
//! [`restore`] what a control was asked to be and the switch that has it
//! written back after firmware has moved it, [`part`] what a device is as
//! a part of the machine, `udev` what udev resolved about one, and
//! [`device`] the devices themselves.

#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    reason = "a workspace-private crate with one caller; its signatures are the contract"
)]

pub(crate) mod build_info;
pub mod device;
pub(crate) mod dmi;
pub(crate) mod drm;
pub(crate) mod ec;
pub(crate) mod edid;
pub(crate) mod gpio;
pub(crate) mod led;
pub(crate) mod lifetime;
pub mod mirror;
pub(crate) mod nvme;
pub(crate) mod panel;
pub mod part;
pub(crate) mod pd;
pub mod restore;
pub(crate) mod sbs;
pub(crate) mod state;
pub(crate) mod touchpad;
pub(crate) mod touchscreen;
pub(crate) mod udev;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
