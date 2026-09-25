//! How [`DeviceError`] crosses the bus: the kind as the D-Bus error name,
//! the sentence as its detail.

use frameguin_contract::DeviceError;

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

/// The kind is read off `fdo::Error`, whose derive already sorts a reply by
/// its error name; a reply outside that vocabulary keeps its sentence alone.
#[must_use]
pub fn from_bus_error(error: zbus::Error) -> DeviceError {
    use zbus::fdo::Error as Fdo;
    match Fdo::from(error) {
        Fdo::InvalidArgs(m) => DeviceError::InvalidArgs(m),
        Fdo::NotSupported(m) => DeviceError::NotSupported(m),
        Fdo::AccessDenied(m) => DeviceError::AccessDenied(m),
        Fdo::UnknownInterface(m) => DeviceError::Absent(m),
        Fdo::Failed(m) => DeviceError::Failed(m),
        Fdo::NoReply(m)
        | Fdo::ServiceUnknown(m)
        | Fdo::NameHasNoOwner(m)
        | Fdo::Timeout(m)
        | Fdo::Disconnected(m) => DeviceError::Unreachable(m),
        Fdo::ZBus(e @ zbus::Error::InputOutput(_)) => DeviceError::Unreachable(cause(&e)),
        Fdo::ZBus(e) => DeviceError::Failed(cause(&e)),
        other => DeviceError::Failed(other.to_string()),
    }
}

#[must_use]
pub fn fdo_error(error: DeviceError) -> zbus::fdo::Error {
    use zbus::fdo::Error as Fdo;
    match error {
        DeviceError::InvalidArgs(m) => Fdo::InvalidArgs(m),
        DeviceError::NotSupported(m) => Fdo::NotSupported(m),
        DeviceError::AccessDenied(m) => Fdo::AccessDenied(m),
        DeviceError::Absent(m) => Fdo::UnknownInterface(m),
        DeviceError::Unreachable(m) | DeviceError::Failed(m) => Fdo::Failed(m),
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::DeviceError;

    use super::{cause, fdo_error, from_bus_error};
    use crate::OBJECT_PATH;

    fn named_error(name: &str, detail: Option<&str>) -> zbus::Error {
        let reply = zbus::Message::method_call(OBJECT_PATH, "SetChargeLimit")
            .unwrap()
            .build(&())
            .unwrap();
        zbus::Error::MethodError(
            name.try_into().unwrap(),
            detail.map(ToString::to_string),
            reply,
        )
    }

    fn method_error(detail: Option<&str>) -> zbus::Error {
        named_error("org.freedesktop.DBus.Error.AccessDenied", detail)
    }

    /// Only an unreachable daemon, which the daemon itself never raises,
    /// comes back as another kind.
    #[test]
    fn every_kind_the_daemon_sends_comes_back_as_itself() {
        for error in [
            DeviceError::InvalidArgs("argument".into()),
            DeviceError::NotSupported("board".into()),
            DeviceError::AccessDenied("prompt".into()),
            DeviceError::Absent("device".into()),
            DeviceError::Failed("hardware".into()),
        ] {
            let crossed = from_bus_error(zbus::Error::from(fdo_error(error.clone())));
            assert_eq!(crossed, error);
        }
        assert_eq!(
            from_bus_error(zbus::Error::from(fdo_error(DeviceError::Unreachable(
                "daemon".into()
            )))),
            DeviceError::Failed("daemon".into())
        );
    }

    #[test]
    fn a_failure_the_daemon_sent_reads_as_its_sentence() {
        let error = named_error("org.freedesktop.DBus.Error.Failed", Some("EC error"));
        assert_eq!(
            from_bus_error(error),
            DeviceError::Failed("EC error".into())
        );
    }

    #[test]
    fn a_daemon_that_never_answered_is_unreachable() {
        let error = named_error("org.freedesktop.DBus.Error.NoReply", Some("no reply"));
        assert_eq!(
            from_bus_error(error),
            DeviceError::Unreachable("no reply".into())
        );
    }

    #[test]
    fn a_refusal_the_daemon_sent_keeps_its_kind() {
        assert_eq!(
            from_bus_error(method_error(Some("not authorized"))),
            DeviceError::AccessDenied("not authorized".into())
        );
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
