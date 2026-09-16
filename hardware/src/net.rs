//! The kernel's net class: an interface's address and its link.

use std::fs;
use std::path::Path;

use frameguin_wire::{LinkState, NetworkLink};

pub(crate) fn link(interface: &Path) -> NetworkLink {
    let (state, megabits) = state(attribute(interface, "carrier").as_deref(), || {
        attribute(interface, "speed")
    });
    NetworkLink {
        interface: interface
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        mac: attribute(interface, "address").unwrap_or_default(),
        state,
        megabits,
    }
}

fn attribute(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name))
        .ok()
        .map(|text| text.trim().to_owned())
}

/// A `carrier` that does not read is an interface that is down; `speed` is
/// answered by the driver rather than the kernel's own record.
fn state(carrier: Option<&str>, speed: impl FnOnce() -> Option<String>) -> (LinkState, u32) {
    match carrier {
        None => (LinkState::Down, 0),
        Some("1") => (
            LinkState::Up,
            speed().and_then(|text| text.parse().ok()).unwrap_or(0),
        ),
        Some(_) => (LinkState::NoCarrier, 0),
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::LinkState;

    use super::state;

    fn unread() -> Option<String> {
        panic!("the rate was asked of a link that is not up")
    }

    #[test]
    fn an_interface_that_is_down_has_no_link() {
        assert_eq!(state(None, unread), (LinkState::Down, 0));
    }

    #[test]
    fn a_link_without_a_carrier_is_not_asked_its_rate() {
        assert_eq!(state(Some("0"), unread), (LinkState::NoCarrier, 0));
    }

    #[test]
    fn a_carried_link_reports_its_rate() {
        assert_eq!(
            state(Some("1"), || Some("1000".to_owned())),
            (LinkState::Up, 1000)
        );
    }

    #[test]
    fn a_rate_the_driver_does_not_know_is_zero() {
        assert_eq!(
            state(Some("1"), || Some("-1".to_owned())),
            (LinkState::Up, 0)
        );
        assert_eq!(state(Some("1"), || None), (LinkState::Up, 0));
    }
}
