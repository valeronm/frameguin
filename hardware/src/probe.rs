//! What this board actually supports.
//!
//! One capability per exposed operation, and each probe must be a
//! side-effect-free exercise of the same code path the operation uses — never
//! a related-but-easier check (a subsystem answers a version read while the
//! command that would act on it works only on other hardware, or is not a
//! command it implements at all — see [`crate::panel::controller`]). Where no
//! harmless same-path probe exists, hardcode the support condition here
//! instead of probing something adjacent. The get-side probes below stand in
//! for their setters only because those EC command pairs ship together in
//! every firmware.
//!
//! A probe decides what to offer, never what to accept: it runs once per
//! daemon lifetime, so one transient read denying a capability would deny it
//! for the whole run. Setters validate against the thing itself.

use frameguin_wire as wire;

use crate::ec::Ec;

/// The controls not yet served as devices of their own; a device answers for
/// itself by being on the bus or not. `ec` is None on hardware with no
/// Framework EC, which leaves every one of them unsupported.
pub fn capabilities(ec: Option<&Ec>) -> Vec<wire::Capability> {
    let mut caps = Vec::new();
    if let Some(ec) = ec
        && ec.keyboard_backlight().is_ok()
    {
        caps.push(wire::Capability::KeyboardBacklight);
    }
    caps
}
