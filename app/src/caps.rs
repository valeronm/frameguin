//! The daemon's probe as the app holds it, and the rows it decides.

use frameguin_wire::{Capability, FpLevel};

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
}

/// Whether the app shows a control the board has. The desktop already carries
/// the keyboard backlight on its own keys and in its own settings, so a second
/// slider only adds another place to set the same value. Filtered at the
/// probe's answer rather than at the widget, so nothing downstream reads,
/// polls or draws it.
fn offered(capability: Capability) -> bool {
    capability != Capability::KeyboardBacklight
}

/// The window's rows: every level this board has, Custom included.
pub(crate) fn fp_rows(caps: Capabilities) -> Vec<FpLevel> {
    FpLevel::ALL
        .into_iter()
        .filter(|level| caps.has(level.requires()))
        .collect()
}

/// The tray's rows: the window's, less the one no click can apply.
pub(crate) fn fp_presets(caps: Capabilities) -> Vec<FpLevel> {
    fp_rows(caps)
        .into_iter()
        .filter(|level| level.is_settable())
        .collect()
}
