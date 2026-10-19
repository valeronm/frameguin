//! Which ports may not be turned off, and why.

use frameguin_contract::{DeviceError, OffRefusal, Platform, Series};

use crate::drm::Displays;

/// Why turning `port` off is refused, and None where it is not, given a read
/// of whether it carries video (None where the port did not answer), a read
/// of the lit displays (None where they did not read), and each port's index
/// with whether its controller has it enabled (None where the mask did not
/// read). Each read is called only where the rest leaves the answer open.
///
/// Only a machine that charges through its ports keeps a port enabled, for a
/// charger. A mask that did not read refuses only where no other port is
/// known to be on. Every machine keeps a port carrying video on while no
/// panel and at most one other output is lit, since no output is placed on a
/// port and the one lit may be the port's own, and keeps on a port whose
/// entry did not answer.
pub(super) fn off_refusal(
    platform: Platform,
    port: u8,
    video: impl FnOnce() -> Option<bool>,
    displays: impl FnOnce() -> Option<Displays>,
    enabled: impl IntoIterator<Item = (u8, Option<bool>)>,
) -> Option<OffRefusal> {
    last_port(platform, port, enabled).or_else(|| last_display(video, displays))
}

fn last_port(
    platform: Platform,
    port: u8,
    enabled: impl IntoIterator<Item = (u8, Option<bool>)>,
) -> Option<OffRefusal> {
    if !charges_through_ports(platform) {
        return None;
    }
    let mut unknown = false;
    for (index, enabled) in enabled {
        match enabled {
            Some(true) if index != port => return None,
            None => unknown = true,
            Some(_) => {}
        }
    }
    Some(if unknown {
        OffRefusal::Unchecked
    } else {
        OffRefusal::LastPort
    })
}

/// A laptop charges through its USB-C ports, every one of which takes a
/// charger; the Desktop runs off a supply of its own.
fn charges_through_ports(platform: Platform) -> bool {
    matches!(
        platform.series(),
        Some(Series::Laptop13 | Series::Laptop12 | Series::Laptop16)
    )
}

/// A refusal the readings settle is `InvalidArgs`; one left open by a
/// reading that did not answer is `Failed`.
pub(super) fn error(refusal: OffRefusal) -> DeviceError {
    match refusal {
        OffRefusal::LastPort => DeviceError::InvalidArgs(
            "the last port on can't be turned off to allow charging".into(),
        ),
        OffRefusal::Unchecked => {
            DeviceError::Failed("a port can't be checked, so this one can't be turned off".into())
        }
        OffRefusal::LastDisplay => DeviceError::InvalidArgs(
            "the port driving the only display on can't be turned off".into(),
        ),
        OffRefusal::DisplaysUnchecked => DeviceError::Failed(
            "the displays can't be checked, so a port driving one can't be turned off".into(),
        ),
    }
}

fn last_display(
    video: impl FnOnce() -> Option<bool>,
    displays: impl FnOnce() -> Option<Displays>,
) -> Option<OffRefusal> {
    match video() {
        None => Some(OffRefusal::Unchecked),
        Some(false) => None,
        Some(true) => match displays() {
            None => Some(OffRefusal::DisplaysUnchecked),
            Some(lit) => (!lit.internal && lit.external <= 1).then_some(OffRefusal::LastDisplay),
        },
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{OffRefusal, Platform};

    use super::off_refusal;
    use crate::drm::Displays;
    use crate::testing::{ONE_MONITOR, PANEL_ONLY};

    const LAPTOP: Platform = Platform::Laptop13ProUltra3;

    const TWO_MONITORS: Option<Displays> = Some(Displays {
        internal: false,
        external: 2,
    });

    const DESKTOP: Platform = Platform::DesktopAmdAiMax300;

    #[test]
    fn a_laptop_keeps_its_last_port_on() {
        let states = [(0, Some(false)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, || Some(false), || PANEL_ONLY, states),
            Some(OffRefusal::LastPort)
        );
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, || Some(false), || PANEL_ONLY, states),
            None
        );
    }

    #[test]
    fn a_laptop_refuses_an_unknown_mask_only_with_no_other_port_on() {
        let states = [(0, Some(false)), (1, Some(true)), (2, None)];
        assert_eq!(
            off_refusal(LAPTOP, 1, || Some(false), || PANEL_ONLY, states),
            Some(OffRefusal::Unchecked)
        );
        let states = [(0, Some(true)), (1, Some(true)), (2, None)];
        assert_eq!(
            off_refusal(LAPTOP, 1, || Some(false), || PANEL_ONLY, states),
            None
        );
    }

    #[test]
    fn a_machine_off_its_own_supply_refuses_no_port_without_video() {
        let states = [(0, Some(false)), (1, Some(true)), (2, None)];
        assert_eq!(
            off_refusal(DESKTOP, 1, || Some(false), || None, states),
            None
        );
        assert_eq!(
            off_refusal(Platform::Unknown, 1, || Some(false), || None, states),
            None
        );
    }

    #[test]
    fn a_port_carrying_the_only_lit_display_stays_on() {
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, || Some(true), || ONE_MONITOR, states),
            Some(OffRefusal::LastDisplay)
        );
        assert_eq!(
            off_refusal(DESKTOP, 1, || Some(true), || ONE_MONITOR, states),
            Some(OffRefusal::LastDisplay)
        );
    }

    #[test]
    fn a_port_carrying_video_goes_off_beside_another_lit_display() {
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, || Some(true), || PANEL_ONLY, states),
            None
        );
        assert_eq!(
            off_refusal(LAPTOP, 1, || Some(true), || TWO_MONITORS, states),
            None
        );
    }

    #[test]
    fn each_read_is_asked_only_where_the_rest_leaves_the_answer_open() {
        let both_on = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, || Some(false), || unreachable!(), both_on),
            None
        );
        let last_on = [(0, Some(false)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, || unreachable!(), || unreachable!(), last_on),
            Some(OffRefusal::LastPort)
        );
    }

    #[test]
    fn a_port_that_does_not_answer_whether_it_carries_video_stays_on() {
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(DESKTOP, 1, || None, || unreachable!(), states),
            Some(OffRefusal::Unchecked)
        );
    }

    #[test]
    fn a_port_carrying_video_stays_on_while_the_displays_are_unknown() {
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(DESKTOP, 1, || Some(true), || None, states),
            Some(OffRefusal::DisplaysUnchecked)
        );
    }
}
