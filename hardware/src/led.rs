//! The power LED through the kernel's LED class — the one control that does
//! not reach the hardware through the EC.
//!
//! Nothing here talks to `framework_lib`: the EC has no off for the power
//! LED (its level command rejects 0, and its BBRAM slot reads a 0 back as
//! full brightness), so off is the kernel holding the LED instead.
//!
//! What the device needs of it is [`LedClass`]; [`Sysfs`] is the class
//! itself.

use std::io;
use std::path::{Path, PathBuf};

/// The kernel's account of the power LED and the two writes that move it,
/// each addressed by the node the account named.
pub trait LedClass: Send + Sync {
    /// The node for a power LED this could take and give back, whatever
    /// state it is in now.
    fn controllable(&self) -> Option<PathBuf>;
    /// That same node, but only while the kernel is holding the LED dark in
    /// the exact arrangement [`LedClass::darken`] leaves.
    fn held_dark(&self) -> Option<PathBuf>;
    /// Takes the LED off the EC's policy and darkens it.
    fn darken(&self, dir: &Path) -> io::Result<()>;
    /// Gives the LED back to the EC.
    fn release(&self, dir: &Path) -> io::Result<()>;
}

/// The LED class under `/sys/class/leds`.
pub struct Sysfs;

impl LedClass for Sysfs {
    fn controllable(&self) -> Option<PathBuf> {
        power_node().map(|(dir, _)| dir)
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
        let (dir, listed) = power_node()?;
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
    fn darken(&self, dir: &Path) -> io::Result<()> {
        std::fs::write(dir.join("trigger"), NO_TRIGGER)?;
        std::fs::write(dir.join("brightness"), "0")
    }

    /// The brightness goes first and only has to be nonzero: the EC reads it
    /// as on-or-off and lights the colour at the level's own duty, so this
    /// restores no value — it stops the kernel's record saying dark once
    /// nothing is holding the LED dark. Writing it after the trigger instead
    /// would be a host command against a LED the EC had just taken back,
    /// undoing the handover.
    fn release(&self, dir: &Path) -> io::Result<()> {
        std::fs::write(dir.join("brightness"), "1")?;
        std::fs::write(dir.join("trigger"), AUTO_TRIGGER)
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

/// The kernel's node for the EC's power LED, with the `trigger` listing that
/// vouched for it — every question asked of that file is answered from the
/// one read.
///
/// This is the LED the EC's `FP_LED` commands dim, and it counts only when it
/// is one this daemon can both darken and hand back. Its name carries the
/// LED's colour, and which colours a power LED has is a board's business, so
/// find it by the function it ends with rather than by one board's spelling
/// of it. A node offering no auto trigger is not a control: it could be
/// darkened and never released.
fn power_node() -> Option<(PathBuf, String)> {
    let dir = std::fs::read_dir("/sys/class/leds")
        .ok()?
        .find_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name = name.to_str()?;
            (name.starts_with("chromeos:") && name.ends_with(":power")).then(|| entry.path())
        })?;
    let listed = std::fs::read_to_string(dir.join("trigger")).ok()?;
    (dir.join("brightness").exists() && triggers(&listed).any(|(name, _)| name == AUTO_TRIGGER))
        .then_some((dir, listed))
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
