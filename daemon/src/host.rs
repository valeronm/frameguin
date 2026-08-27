//! Facts about this run of the machine, where [`crate::board`] answers for
//! the machine itself.
//!
//! What a mirror is dated against when the thing holding the state is reset
//! by a reboot. [`crate::ec`]'s stamp answers a different question — whether
//! the EC has restarted — and the two never substitute for each other, the EC
//! running straight through a host reboot. Which one a mirror wants is
//! [`crate::state`]'s to say, since it is decided by what holds the value.

use std::sync::OnceLock;

/// The kernel's name for this boot, fresh on every one.
///
/// Memoized: it cannot change while this process lives, and reading it is a
/// file open that every dated write would otherwise repeat.
///
/// None where the host will not say — `/proc/sys` hidden from this unit by
/// `ProcSubset=`, or a policy refusing the read. What that means is the
/// caller's to decide, and the two callers here decide it differently: see
/// [`BootStamp::now`] and [`BootStamp::still_current`].
fn boot() -> Option<&'static str> {
    static BOOT: OnceLock<Option<String>> = OnceLock::new();
    BOOT.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()
            .map(|id| id.trim().to_owned())
    })
    .as_deref()
}

/// A write dated against the life of this boot, for state the hardware loses
/// when the machine restarts.
#[derive(Clone)]
pub(crate) struct BootStamp(String);

impl BootStamp {
    /// Dates a write about to be made, and None where this host will not name
    /// its boot. A caller takes that as losing the record and never the write:
    /// what a stamp buys is knowing later whether a value still stands, which
    /// is not worth refusing a write the hardware would have taken.
    pub(crate) fn now() -> Option<Self> {
        boot().map(|id| Self(id.to_owned()))
    }

    /// A stamp as a state file carries it, parsed and not yet weighed —
    /// [`Self::still_current`] is what weighs one, and a caller that reads a
    /// stamp owes that call before believing it.
    pub(crate) fn stored(id: &str) -> Self {
        Self(id.to_owned())
    }

    /// Whether the machine has been running without a restart since this was
    /// taken — which is to say whether whatever it dated is still there.
    ///
    /// False where the host will not name its boot. A stamp that cannot be
    /// weighed is one there is no reason to believe, and what these date is
    /// always a claim a reader acts on rather than merely looks at.
    pub(crate) fn still_current(&self) -> bool {
        self.matches(boot())
    }

    /// The comparison itself, against a boot named rather than looked up, so
    /// what decides whether a mirror survives can be exercised without one.
    fn matches(&self, boot: Option<&str>) -> bool {
        boot == Some(self.0.as_str())
    }

    /// The stamp as a state file should carry it.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::BootStamp;

    const BOOT: &str = "00000000-0000-4000-8000-000000000001";
    const EARLIER: &str = "00000000-0000-4000-8000-000000000002";

    #[test]
    fn a_stamp_naming_this_boot_still_stands() {
        assert!(BootStamp::stored(BOOT).matches(Some(BOOT)));
    }

    /// The case the dating exists for: whatever the stamp vouched for was
    /// reset by the restart, so the mirror has to stop claiming it.
    #[test]
    fn a_stamp_from_an_earlier_boot_does_not() {
        assert!(!BootStamp::stored(EARLIER).matches(Some(BOOT)));
    }

    /// A host that will not name its boot cannot weigh a stamp against it,
    /// and an unweighable stamp is one there is no reason to believe.
    #[test]
    fn a_boot_this_host_cannot_name_withdraws_every_stamp() {
        assert!(!BootStamp::stored(BOOT).matches(None));
    }
}
