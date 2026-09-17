//! The EC's LEDs through the kernel's LED class, which is how either is
//! switched off.
//!
//! Nothing here talks to `framework_lib`: the EC has no off for the power
//! LED (its level command rejects 0, and its BBRAM slot reads a 0 back as
//! full brightness), so off is the kernel holding the LED instead.
//!
//! What a device needs of it is [`LedClass`]; [`Sysfs`] is the class
//! itself.

use std::path::{Path, PathBuf};

use frameguin_wire::{DeviceError, DeviceResult};

/// The kernel's account of one LED and the two writes that move it, each
/// addressed by the node the account named.
pub trait LedClass: Send + Sync {
    /// The node for the LED this could take and give back, whatever state it
    /// is in now.
    fn controllable(&self) -> Option<PathBuf>;
    /// That same node, but only while the kernel is holding the LED dark in
    /// the exact arrangement [`LedClass::darken`] leaves.
    fn held_dark(&self) -> Option<PathBuf>;
    /// Takes the LED off the EC's policy and darkens it.
    fn darken(&self, dir: &Path) -> DeviceResult<()>;
    /// Gives the LED back to the EC.
    fn release(&self, dir: &Path) -> DeviceResult<()>;

    /// Refused where the kernel has no node to hold the LED with.
    fn hold_dark(&self) -> DeviceResult<()> {
        let dir = self.controllable().ok_or_else(|| {
            DeviceError::NotSupported("the kernel has no node to hold this LED with".into())
        })?;
        self.darken(&dir)
    }

    /// Releases only an LED held dark: one parked lit on another trigger is
    /// somebody else's.
    fn release_held(&self) -> DeviceResult<()> {
        self.held_dark().map_or(Ok(()), |dir| self.release(&dir))
    }
}

/// One of the EC's LEDs under `/sys/class/leds`, None where the kernel has
/// no node for it that could be darkened and handed back.
///
/// Found once: the kernel names the node when its driver probes, and the only
/// re-probe is a reboot, which the daemon does not outlive.
pub(crate) struct Sysfs {
    dir: Option<PathBuf>,
    released_at: &'static str,
}

impl Sysfs {
    /// Released at a nonzero brightness, so the kernel's record stops saying
    /// dark once nothing holds the LED dark; the power LED's one colour lights
    /// at the duty its level already set.
    pub(crate) fn power() -> Self {
        Self::find("power", "1")
    }

    /// Released at zero: a nonzero write lights the first colour on both
    /// sides until the EC's policy takes the LED back on its next tick. The
    /// record left at zero is not read as dark, the trigger no longer being
    /// `none`.
    pub(crate) fn charging() -> Self {
        Self::find("charging", "0")
    }

    /// The node's name carries the LED's colour, and which colours an LED
    /// has is a board's business, so it is found by the function it ends with
    /// rather than by one board's spelling of it. A node offering no auto
    /// trigger is not a control: it could be darkened and never released.
    fn find(function: &str, released_at: &'static str) -> Self {
        let suffix = format!(":{function}");
        let dir = std::fs::read_dir("/sys/class/leds")
            .ok()
            .and_then(|entries| {
                entries.filter_map(Result::ok).find_map(|entry| {
                    let name = entry.file_name();
                    let name = name.to_str()?;
                    (name.starts_with("chromeos:") && name.ends_with(&suffix)).then(|| entry.path())
                })
            })
            .filter(|dir| {
                dir.join("brightness").exists()
                    && std::fs::read_to_string(dir.join("trigger"))
                        .is_ok_and(|listed| triggers(&listed).any(|(name, _)| name == AUTO_TRIGGER))
            });
        Self { dir, released_at }
    }
}

impl LedClass for Sysfs {
    fn controllable(&self) -> Option<PathBuf> {
        self.dir.clone()
    }

    /// A LED parked on some third trigger is somebody else's and not ours to
    /// read as off.
    ///
    /// This is the kernel's record of what it last commanded rather than a
    /// reading: the driver implements no `brightness_get`, and the EC's LED
    /// command answers only with which colours exist. So a write that goes
    /// straight to the EC (`ectool led`) passes unseen, while a host reboot
    /// re-probes the driver and re-attaches the trigger, which reads as on.
    fn held_dark(&self) -> Option<PathBuf> {
        let dir = self.dir.clone()?;
        let listed = std::fs::read_to_string(dir.join("trigger")).ok()?;
        let held_dark = active_in(&listed) == Some(NO_TRIGGER)
            && std::fs::read_to_string(dir.join("brightness"))
                .is_ok_and(|value| value.trim() == "0");
        held_dark.then_some(dir)
    }

    /// Through the kernel rather than by sending `EC_CMD_LED_CONTROL` to the
    /// EC directly, which would work and which the daemon is otherwise
    /// equipped to do: the EC keeps no readable record of who owns the LED,
    /// so the driver's is the only one there is, and a command issued behind
    /// its back would leave it describing a policy the EC had already stopped
    /// following. Detaching the trigger before the brightness write is that
    /// same argument a level down — the trigger has no deactivate handler and
    /// never re-asserts, so a write underneath one leaves the file naming a
    /// policy no longer in force.
    fn darken(&self, dir: &Path) -> DeviceResult<()> {
        std::fs::write(dir.join("trigger"), NO_TRIGGER)?;
        std::fs::write(dir.join("brightness"), "0")?;
        Ok(())
    }

    /// The brightness goes first: the EC reads it as on-or-off and lights the
    /// colour at the duty it holds for that colour, so this restores no value.
    /// Writing it after the trigger instead would be a host command against a
    /// LED the EC had just taken back, undoing the handover.
    fn release(&self, dir: &Path) -> DeviceResult<()> {
        std::fs::write(dir.join("brightness"), self.released_at)?;
        std::fs::write(dir.join("trigger"), AUTO_TRIGGER)?;
        Ok(())
    }
}

/// The EC's own LED policy, under the name the kernel's LED class gives it.
/// Handing the LED back is done by activating this trigger: the activate
/// handler is what sends the EC its auto flag.
const AUTO_TRIGGER: &str = "chromeos-auto";

/// No policy at all — the LED left to whatever brightness was last written.
const NO_TRIGGER: &str = "none";

/// A `trigger` file's listing: every trigger the kernel offers, each paired
/// with whether it is the one in effect — which the file marks, and marks
/// only, by bracketing it. One decoding of that convention, so no two
/// questions asked of the file can come to disagree about it.
fn triggers(listed: &str) -> impl Iterator<Item = (&str, bool)> {
    listed.split_whitespace().map(|token| {
        token
            .strip_prefix('[')
            .and_then(|name| name.strip_suffix(']'))
            .map_or((token, false), |name| (name, true))
    })
}

fn active_in(listed: &str) -> Option<&str> {
    triggers(listed).find_map(|(name, active)| active.then_some(name))
}

#[cfg(test)]
mod tests {
    use super::{AUTO_TRIGGER, NO_TRIGGER, active_in, triggers};

    /// A `trigger` file as the kernel writes it, shortened. Which trigger is
    /// in effect is carried by brackets and nothing else, so the parsing is
    /// all that stands between a LED handed back to the EC and one only
    /// believed to be.
    const LISTED: &str = "none default rfkill-any panic chromeos-auto phy0rx";

    #[test]
    fn the_active_trigger_is_the_bracketed_one() {
        assert_eq!(
            active_in(&LISTED.replace("chromeos-auto", "[chromeos-auto]")),
            Some(AUTO_TRIGGER)
        );
        assert_eq!(
            active_in(&LISTED.replace("none", "[none]")),
            Some(NO_TRIGGER)
        );
    }

    /// Nothing bracketed means the kernel named no trigger, which is not the
    /// same as it naming the one called "none".
    #[test]
    fn a_listing_marking_nothing_has_no_active_trigger() {
        assert_eq!(active_in(LISTED), None);
    }

    #[test]
    fn a_trigger_is_offered_whether_or_not_it_is_the_active_one() {
        let active = LISTED.replace("chromeos-auto", "[chromeos-auto]");
        assert!(triggers(&active).any(|(name, _)| name == AUTO_TRIGGER));
        assert!(triggers(LISTED).any(|(name, _)| name == AUTO_TRIGGER));
    }
}
