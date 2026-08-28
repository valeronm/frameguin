//! What holds a mirrored value, and how to tell later that the holder is
//! still the one that took it. Two holders: the EC, and the host's own power
//! state.

/// Whose life a mirrored value shares.
#[derive(Clone, Copy)]
pub enum Lifetime {
    /// The hardware keeps the value itself, so nothing withdraws it.
    Permanent,
    /// The EC's RAM, cleared by an EC restart and by nothing else.
    Ec,
    /// This boot of the host, and only until it next sleeps.
    HostAwake,
}

/// Each holder's boot, read once per run and None where the holder will not
/// say; the host's sleep is the one reading taken as it is weighed rather
/// than handed in here.
#[derive(Clone)]
pub struct Holders {
    ec: Option<EcBoot>,
    host: Option<String>,
}

impl Holders {
    pub fn new(ec: Option<EcBoot>, host: Option<String>) -> Self {
        Self { ec, host }
    }
}

/// That the holder a [`Lifetime`] names is still the one that took a write.
pub(crate) enum Evidence {
    Standing,
    Ec(u64),
    Host(HostWaking),
}

impl Lifetime {
    /// The evidence a write made now would carry.
    pub(crate) fn witness(self, holders: &Holders) -> Option<Evidence> {
        match self {
            Self::Permanent => Some(Evidence::Standing),
            Self::Ec => holders.ec.map(|boot| Evidence::Ec(boot.booted_at)),
            Self::HostAwake => HostWaking::now(holders).map(Evidence::Host),
        }
    }

    /// The evidence a store carries for this lifetime, from what
    /// [`Evidence::stored`] put there.
    pub(crate) fn recall(self, stored: Option<&str>) -> Option<Evidence> {
        match self {
            Self::Permanent => Some(Evidence::Standing),
            Self::Ec => stored?.parse().ok().map(Evidence::Ec),
            Self::HostAwake => HostWaking::parse(stored?).map(Evidence::Host),
        }
    }
}

impl Evidence {
    pub(crate) fn proves(&self, holders: &Holders) -> bool {
        match self {
            Self::Standing => true,
            Self::Ec(booted_at) => holders.ec.is_some_and(|boot| boot.same_as(*booted_at)),
            Self::Host(waking) => waking.matches(holders.host.as_deref(), suspended_ms()),
        }
    }

    pub(crate) fn stored(&self) -> Option<String> {
        match self {
            Self::Standing => None,
            Self::Ec(booted_at) => Some(booted_at.to_string()),
            Self::Host(waking) => Some(waking.stored()),
        }
    }
}

/// The EC's boot as the wall time it happened at, which is the one thing
/// about its life a reading from another run can be compared with: the EC
/// answers its uptime and nothing with an identity.
#[derive(Clone, Copy)]
pub struct EcBoot {
    booted_at: u64,
    uptime: u64,
}

impl EcBoot {
    /// Both readings taken at one moment, which is what makes them
    /// subtractable.
    pub const fn from_clocks(uptime: u64, now: u64) -> Self {
        Self {
            booted_at: now.saturating_sub(uptime),
            uptime,
        }
    }

    /// Whether `booted_at` names this same boot. The slack absorbs the EC
    /// clock's 1% frequency error against the host's, so it grows with the
    /// uptime; only a later boot is a restart, an earlier one being noise.
    fn same_as(self, booted_at: u64) -> bool {
        self.booted_at.saturating_sub(booted_at) <= (self.uptime / 20).max(60)
    }
}

/// One waking of the host: the boot it belongs to and the time that boot
/// had spent asleep when the write was made, which the next sleep advances.
pub(crate) struct HostWaking {
    boot: String,
    suspended_ms: u64,
}

/// The suspended time is two clock reads apart rather than one instant, so it
/// jitters by however long that takes.
const SUSPEND_FLOOR_MS: u64 = 100;

impl HostWaking {
    fn now(holders: &Holders) -> Option<Self> {
        Some(Self {
            boot: holders.host.clone()?,
            suspended_ms: suspended_ms()?,
        })
    }

    fn stored(&self) -> String {
        format!("{}:{}", self.boot, self.suspended_ms)
    }

    fn parse(value: &str) -> Option<Self> {
        let (boot, suspended_ms) = value.rsplit_once(':')?;
        Some(Self {
            boot: boot.to_owned(),
            suspended_ms: suspended_ms.parse().ok()?,
        })
    }

    /// Readings are named rather than looked up, so the rule can be exercised
    /// without a machine to suspend.
    fn matches(&self, boot: Option<&str>, suspended_ms: Option<u64>) -> bool {
        boot == Some(self.boot.as_str())
            && suspended_ms
                .is_some_and(|ms| ms.saturating_sub(self.suspended_ms) < SUSPEND_FLOOR_MS)
    }
}

/// None where `/proc/sys` is hidden from this unit by `ProcSubset=`.
pub fn host_boot() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|id| id.trim().to_owned())
}

/// How long this boot has spent suspended, which is the one thing the two
/// clocks differ by.
fn suspended_ms() -> Option<u64> {
    Some(clock_ms(libc::CLOCK_BOOTTIME)?.saturating_sub(clock_ms(libc::CLOCK_MONOTONIC)?))
}

fn clock_ms(clock: libc::clockid_t) -> Option<u64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: the kernel writes one `timespec` through the pointer, which is
    // a live local for the length of the call.
    let rc = unsafe { libc::clock_gettime(clock, &raw mut time) };
    if rc != 0 {
        return None;
    }
    let seconds = u64::try_from(time.tv_sec).ok()?;
    let nanoseconds = u64::try_from(time.tv_nsec).ok()?;
    Some(seconds * 1000 + nanoseconds / 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::{EcBoot, Evidence, Holders, HostWaking, Lifetime, SUSPEND_FLOOR_MS};

    const BOOT: &str = "00000000-0000-4000-8000-000000000001";
    const EARLIER: &str = "00000000-0000-4000-8000-000000000002";
    const HOUR: u64 = 3_600;
    const DAY: u64 = 24 * HOUR;

    const WRITTEN: EcBoot = EcBoot::from_clocks(500_000, 1_000_000);

    #[test]
    fn a_write_moments_ago_is_still_the_same_boot() {
        assert!(EcBoot::from_clocks(500_002, 1_000_002).same_as(WRITTEN.booted_at));
    }

    #[test]
    fn an_ec_that_has_run_the_elapsed_time_is_still_the_same_boot() {
        assert!(EcBoot::from_clocks(500_000 + DAY, 1_000_000 + DAY).same_as(WRITTEN.booted_at));
    }

    #[test]
    fn an_ec_that_restarted_is_a_different_boot() {
        assert!(!EcBoot::from_clocks(60, 1_000_000 + HOUR).same_as(WRITTEN.booted_at));
    }

    #[test]
    fn clock_drift_over_a_long_uptime_is_not_a_restart() {
        let written = EcBoot::from_clocks(0, 1_000_000);
        let elapsed = 10 * DAY;
        let one_percent_short = elapsed - elapsed / 100;
        assert!(
            EcBoot::from_clocks(one_percent_short, 1_000_000 + elapsed).same_as(written.booted_at)
        );
    }

    #[test]
    fn a_recent_write_gets_the_floor_not_the_percentage() {
        let written = EcBoot::from_clocks(10, 1_000_000);
        assert!(EcBoot::from_clocks(10, 1_000_000 + 30).same_as(written.booted_at));
        assert!(!EcBoot::from_clocks(10, 1_000_000 + 200).same_as(written.booted_at));
    }

    #[test]
    fn evidence_survives_the_round_trip_through_a_store() {
        let stored = Evidence::Ec(500_000).stored();
        assert!(matches!(
            Lifetime::Ec.recall(stored.as_deref()),
            Some(Evidence::Ec(500_000))
        ));
        let stored = Evidence::Host(host(5_000)).stored();
        assert!(matches!(
            Lifetime::HostAwake.recall(stored.as_deref()),
            Some(Evidence::Host(waking)) if waking.matches(Some(BOOT), Some(5_000))
        ));
        assert!(matches!(
            Lifetime::Permanent.recall(None),
            Some(Evidence::Standing)
        ));
        assert!(Lifetime::Ec.recall(None).is_none());
        assert!(Lifetime::Ec.recall(Some("tomorrow")).is_none());
        assert!(Lifetime::HostAwake.recall(Some(BOOT)).is_none());
    }

    #[test]
    fn a_holder_that_will_not_say_neither_witnesses_nor_proves() {
        let holders = Holders::new(None, None);
        assert!(Lifetime::Ec.witness(&holders).is_none());
        assert!(Lifetime::HostAwake.witness(&holders).is_none());
        assert!(!Evidence::Ec(500_000).proves(&holders));
        assert!(!Evidence::Host(host(0)).proves(&holders));
        assert!(Evidence::Standing.proves(&holders));
    }

    fn host_at(boot: &str, suspended_ms: u64) -> HostWaking {
        HostWaking {
            boot: boot.to_owned(),
            suspended_ms,
        }
    }

    fn host(suspended_ms: u64) -> HostWaking {
        host_at(BOOT, suspended_ms)
    }

    #[test]
    fn a_waking_of_this_boot_with_no_sleep_since_still_stands() {
        assert!(host(5_000).matches(Some(BOOT), Some(5_000)));
    }

    #[test]
    fn a_sleep_since_the_write_ends_the_waking() {
        assert!(!host(5_000).matches(Some(BOOT), Some(65_000)));
    }

    #[test]
    fn jitter_below_the_floor_is_not_a_sleep() {
        assert!(host(5_000).matches(Some(BOOT), Some(5_000 + SUSPEND_FLOOR_MS - 1)));
        assert!(!host(5_000).matches(Some(BOOT), Some(5_000 + SUSPEND_FLOOR_MS)));
    }

    #[test]
    fn a_waking_of_an_earlier_boot_does_not_stand() {
        assert!(!host_at(EARLIER, 5_000).matches(Some(BOOT), Some(0)));
    }

    #[test]
    fn a_host_that_will_not_answer_ends_every_waking() {
        assert!(!host(5_000).matches(None, Some(5_000)));
        assert!(!host(5_000).matches(Some(BOOT), None));
    }
}
