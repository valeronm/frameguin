//! The one error every implementation of the control traits raises.

use std::fmt;

/// What a failed operation says, by kind — so a caller can tell an argument
/// it got wrong from hardware that is not there from a prompt that was
/// declined from a daemon that never answered — and the sentence for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceError {
    InvalidArgs(String),
    /// The hardware that is there cannot do this — no EC on the board, no
    /// route to the panel. A device that is present raises it.
    NotSupported(String),
    AccessDenied(String),
    /// No such device. Only the bus implementation raises it, from the bus's
    /// unknown-interface reply: the daemon registers a device's interface
    /// only where it detected the device, so that reply is the device's
    /// absence and nothing else, and a device's `detect` reads it as such.
    Absent(String),
    /// The daemon did not answer at all, which only the bus implementation
    /// can tell.
    Unreachable(String),
    Failed(String),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgs(m)
            | Self::NotSupported(m)
            | Self::AccessDenied(m)
            | Self::Absent(m)
            | Self::Unreachable(m)
            | Self::Failed(m) => f.write_str(m),
        }
    }
}

impl From<std::io::Error> for DeviceError {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

pub type DeviceResult<T> = Result<T, DeviceError>;
