//! The one error every implementation of the control traits raises, and how
//! it crosses the bus.

use std::fmt;

/// What a failed call says, without the D-Bus error name in front of it.
///
/// The two ends meet in this as much as in the vocabularies, and again
/// neither can see the other's half: the daemon puts a sentence a reader can
/// act on in the error's detail — "not authorized", "no battery present" —
/// and zbus renders the pair as "{name}: {detail}", the name being machine
/// vocabulary in front of it. Taking the detail alone is what makes writing
/// that half worth the daemon's trouble. Anything but a method error renders
/// whole, having no better half to show.
fn cause(error: &zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(_, Some(detail), _) => detail.clone(),
        other => other.to_string(),
    }
}

/// What a failed operation says, by the kind the daemon's interface answers
/// with — so a caller can tell an argument it got wrong from hardware that
/// is not there from a prompt that was declined — and the sentence for it.
///
/// The one error every implementation of the control traits raises. The
/// direct implementation raises the kind itself; over the bus the kind
/// travels as the D-Bus error name and the sentence as its detail, and
/// [`DeviceError::from`] a `zbus::Error` puts the two back together.
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
    Failed(String),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgs(m)
            | Self::NotSupported(m)
            | Self::AccessDenied(m)
            | Self::Absent(m)
            | Self::Failed(m) => f.write_str(m),
        }
    }
}

/// The kind is read off `fdo::Error`, whose derive already sorts a reply by
/// its error name; a reply outside that vocabulary keeps its sentence alone.
impl From<zbus::Error> for DeviceError {
    fn from(error: zbus::Error) -> Self {
        use zbus::fdo::Error as Fdo;
        match Fdo::from(error) {
            Fdo::InvalidArgs(m) => Self::InvalidArgs(m),
            Fdo::NotSupported(m) => Self::NotSupported(m),
            Fdo::AccessDenied(m) => Self::AccessDenied(m),
            Fdo::UnknownInterface(m) => Self::Absent(m),
            Fdo::ZBus(e) => Self::Failed(cause(&e)),
            other => Self::Failed(other.to_string()),
        }
    }
}

impl From<std::io::Error> for DeviceError {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<DeviceError> for zbus::fdo::Error {
    fn from(error: DeviceError) -> Self {
        match error {
            DeviceError::InvalidArgs(m) => Self::InvalidArgs(m),
            DeviceError::NotSupported(m) => Self::NotSupported(m),
            DeviceError::AccessDenied(m) => Self::AccessDenied(m),
            DeviceError::Absent(m) => Self::UnknownInterface(m),
            DeviceError::Failed(m) => Self::Failed(m),
        }
    }
}

pub type DeviceResult<T> = Result<T, DeviceError>;

#[cfg(test)]
mod tests {
    use super::cause;
    use crate::vocabulary::OBJECT_PATH;

    fn method_error(detail: Option<&str>) -> zbus::Error {
        let reply = zbus::Message::method_call(OBJECT_PATH, "SetChargeLimit")
            .unwrap()
            .build(&())
            .unwrap();
        zbus::Error::MethodError(
            "org.freedesktop.DBus.Error.AccessDenied"
                .try_into()
                .unwrap(),
            detail.map(ToString::to_string),
            reply,
        )
    }

    /// Declining the polkit prompt is the failure every user meets, and the
    /// daemon's own half of it is already the sentence they need.
    #[test]
    fn a_method_error_reads_as_its_detail_alone() {
        assert_eq!(
            cause(&method_error(Some("not authorized"))),
            "not authorized"
        );
    }

    /// The name is all there is where a reply carries no detail, so it stays
    /// rather than leaving the sentence trailing nothing.
    #[test]
    fn an_error_without_detail_keeps_what_it_has() {
        assert!(cause(&method_error(None)).contains("AccessDenied"));
    }
}
