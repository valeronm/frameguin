//! The daemon's probe as the app holds it, and the rows it decides.

use frameguin_wire::{Capability, PowerLedLevel};

use crate::format::power_led_row_rank;

/// What this app offers on the connected board: what the daemon's probe
/// answered with, less what [`offered`] declines to show. Kept as one bit per
/// [`Capability`] rather than unpacked into a flag apiece, so a new control is
/// named in the vocabulary and gated in the UI with nothing to update in
/// between. `Copy`, because the window holds it in a `Cell` and the tray in
/// its own copy. The default (empty) doubles as "not yet known".
#[derive(Clone, Copy, Default)]
pub(crate) struct Capabilities(u32);

impl Capabilities {
    pub(crate) fn from_probe(probed: &[Capability]) -> Self {
        Capabilities(
            probed
                .iter()
                .copied()
                .filter(|&capability| offered(capability))
                .fold(0, |bits, capability| bits | 1 << capability as u32),
        )
    }

    pub(crate) fn has(self, capability: Capability) -> bool {
        self.0 & (1 << capability as u32) != 0
    }
    /// Nothing to offer on this board. True as well before a probe has
    /// answered, so ask it only where an answer has arrived.
    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Whether the app shows a control the board has. The desktop already carries
/// the keyboard backlight on its own keys and in its own settings, so a second
/// slider only adds another place to set the same value. Filtered at the
/// probe's answer rather than at the widget, so nothing downstream reads,
/// polls or draws it.
fn offered(capability: Capability) -> bool {
    capability != Capability::KeyboardBacklight
}

/// The window's rows: every level this board has, Custom included, in the
/// order [`power_led_row_rank`] gives them. Both front-ends come through
/// here, which is what keeps them from drawing the rows two ways.
pub(crate) fn power_led_rows(caps: Capabilities) -> Vec<PowerLedLevel> {
    let mut rows: Vec<PowerLedLevel> = PowerLedLevel::ALL
        .into_iter()
        .filter(|level| caps.has(level.requires()))
        .collect();
    rows.sort_unstable_by_key(|&level| power_led_row_rank(level));
    rows
}

/// The tray's rows: the window's, less the one no click can apply.
pub(crate) fn power_led_presets(caps: Capabilities) -> Vec<PowerLedLevel> {
    power_led_rows(caps)
        .into_iter()
        .filter(|level| level.is_settable())
        .collect()
}
