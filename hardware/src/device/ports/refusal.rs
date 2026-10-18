//! Which ports may not be turned off, and why.

use frameguin_contract::{DeviceError, OffRefusal, Platform, Series};

use crate::drm::Displays;

/// Why turning `port` off is refused, and None where it is not, given
/// whether it carries video, a read of the lit displays (None where they did
/// not read), and each port's index with whether its controller has it
/// enabled (None where the mask did not read). `displays` is called only
/// where the rest leaves the answer open.
///
/// Only a machine that charges through its ports keeps a port enabled, for a
/// charger. Every machine keeps a port carrying video on while no panel and
/// at most one other output is lit: no output is placed on a port, so the
/// one lit may be the port's own.
pub(super) fn off_refusal(
    platform: Platform,
    port: u8,
    video: bool,
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
    let mut another_on = false;
    for (index, enabled) in enabled {
        match enabled {
            None => return Some(OffRefusal::Unchecked),
            Some(on) => another_on |= on && index != port,
        }
    }
    (!another_on).then_some(OffRefusal::LastPort)
}

/// A laptop charges through its USB-C ports, every one of which takes a
/// charger; the Desktop runs off a supply of its own.
fn charges_through_ports(platform: Platform) -> bool {
    matches!(
        platform.series(),
        Some(Series::Laptop13 | Series::Laptop12 | Series::Laptop16)
    )
}

/// A refusal the readings settle is `InvalidArgs`; one they leave open, a
/// mask or the displays unread, is `Failed`.
pub(super) fn error(refusal: OffRefusal) -> DeviceError {
    match refusal {
        OffRefusal::LastPort => DeviceError::InvalidArgs(
            "the last port on can't be turned off to allow charging".into(),
        ),
        OffRefusal::Unchecked => {
            DeviceError::Failed("a port can't be checked, so none can be turned off".into())
        }
        OffRefusal::LastDisplay => DeviceError::InvalidArgs(
            "the port driving the only display on can't be turned off".into(),
        ),
        OffRefusal::DisplaysUnchecked => DeviceError::Failed(
            "the displays can't be checked, so a port driving one can't be turned off".into(),
        ),
    }
}

fn last_display(video: bool, displays: impl FnOnce() -> Option<Displays>) -> Option<OffRefusal> {
    if !video {
        return None;
    }
    match displays() {
        None => Some(OffRefusal::DisplaysUnchecked),
        Some(lit) => (!lit.internal && lit.external <= 1).then_some(OffRefusal::LastDisplay),
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
            off_refusal(LAPTOP, 1, false, || PANEL_ONLY, states),
            Some(OffRefusal::LastPort)
        );
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(off_refusal(LAPTOP, 1, false, || PANEL_ONLY, states), None);
    }

    #[test]
    fn a_laptop_refuses_while_any_mask_is_unknown() {
        let states = [(0, Some(true)), (1, Some(true)), (2, None)];
        assert_eq!(
            off_refusal(LAPTOP, 1, false, || PANEL_ONLY, states),
            Some(OffRefusal::Unchecked)
        );
    }

    #[test]
    fn a_machine_off_its_own_supply_refuses_no_port_without_video() {
        let states = [(0, Some(false)), (1, Some(true)), (2, None)];
        assert_eq!(off_refusal(DESKTOP, 1, false, || None, states), None);
        assert_eq!(
            off_refusal(Platform::Unknown, 1, false, || None, states),
            None
        );
    }

    #[test]
    fn a_port_carrying_the_only_lit_display_stays_on() {
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, true, || ONE_MONITOR, states),
            Some(OffRefusal::LastDisplay)
        );
        assert_eq!(
            off_refusal(DESKTOP, 1, true, || ONE_MONITOR, states),
            Some(OffRefusal::LastDisplay)
        );
    }

    #[test]
    fn a_port_carrying_video_goes_off_beside_another_lit_display() {
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(off_refusal(LAPTOP, 1, true, || PANEL_ONLY, states), None);
        assert_eq!(off_refusal(LAPTOP, 1, true, || TWO_MONITORS, states), None);
    }

    #[test]
    fn the_displays_are_read_only_where_the_rest_leaves_the_answer_open() {
        let both_on = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, false, || unreachable!(), both_on),
            None
        );
        let last_on = [(0, Some(false)), (1, Some(true))];
        assert_eq!(
            off_refusal(LAPTOP, 1, true, || unreachable!(), last_on),
            Some(OffRefusal::LastPort)
        );
    }

    #[test]
    fn a_port_carrying_video_stays_on_while_the_displays_are_unknown() {
        let states = [(0, Some(true)), (1, Some(true))];
        assert_eq!(
            off_refusal(DESKTOP, 1, true, || None, states),
            Some(OffRefusal::DisplaysUnchecked)
        );
    }
}
