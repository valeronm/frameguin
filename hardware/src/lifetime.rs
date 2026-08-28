//! Dating a mirrored write against whatever holds the state it claims, and
//! weighing it later. Two holders: the EC, and the host's own power state.

use std::sync::OnceLock;

/// A write dated against the EC's life, as seconds of EC uptime paired with
/// the wall time of that same moment.
#[derive(Clone, Copy, Default)]
pub struct EcStamp {
    ec_uptime: u64,
    written_at: u64,
}

impl EcStamp {
    /// Both readings are [`crate::ec`]'s to take, that module holding every
    /// EC call.
    pub fn taken(ec_uptime: u64, written_at: u64) -> Self {
        Self {
            ec_uptime,
            written_at,
        }
    }

    /// The slack absorbs the EC clock's 1% frequency error against the host's.
    pub fn same_boot(self, ec_uptime: u64, now: u64) -> bool {
        let expected = self.ec_uptime + now.saturating_sub(self.written_at);
        expected.saturating_sub(ec_uptime) <= (expected / 20).max(60)
    }

    /// Whether what was written still stands, weighed against the EC's
    /// uptime and the wall time read together. False where the EC would not
    /// answer, an unweighable stamp being one there is no reason to believe.
    pub fn still_current(self, clocks: Option<(u64, u64)>) -> bool {
        clocks.is_some_and(|(ec_uptime, now)| self.same_boot(ec_uptime, now))
    }

    pub fn stored(self) -> String {
        format!("{}:{}", self.ec_uptime, self.written_at)
    }

    pub fn parse(value: &str) -> Option<Self> {
        let (uptime, written_at) = value.rsplit_once(':')?;
        Some(Self::taken(uptime.parse().ok()?, written_at.parse().ok()?))
    }
}

/// A write dated against this host's power state, by the boot it was made in
/// and the time that boot had spent asleep.
#[derive(Clone)]
pub struct HostStamp {
    boot: String,
    suspended_ms: u64,
}

/// The suspended time is two clock reads apart rather than one instant, so it
/// jitters by however long that takes.
const SUSPEND_FLOOR_MS: u64 = 100;

impl HostStamp {
    /// None where the host will not name its boot, which costs the record and
    /// never the write.
    pub fn now() -> Option<Self> {
        Some(Self {
            boot: boot()?.to_owned(),
            suspended_ms: suspended_ms()?,
        })
    }

    /// False where the host will not answer, an unweighable stamp being one
    /// there is no reason to believe.
    pub fn still_current(&self) -> bool {
        self.matches(boot(), suspended_ms())
    }

    pub fn stored(&self) -> String {
        format!("{}:{}", self.boot, self.suspended_ms)
    }

    pub fn parse(value: &str) -> Option<Self> {
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

/// Memoized because it cannot change while this process lives. None where
/// `/proc/sys` is hidden from this unit by `ProcSubset=`.
fn boot() -> Option<&'static str> {
    static BOOT: OnceLock<Option<String>> = OnceLock::new();
    BOOT.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()
            .map(|id| id.trim().to_owned())
    })
    .as_deref()
}

/// How long this boot has spent suspended, which is the one thing the two
/// clocks differ by. Never memoized: moving is what it is read for.
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
    use super::{EcStamp, HostStamp, SUSPEND_FLOOR_MS};

    const BOOT: &str = "00000000-0000-4000-8000-000000000001";
    const EARLIER: &str = "00000000-0000-4000-8000-000000000002";
    const HOUR: u64 = 3_600;
    const DAY: u64 = 24 * HOUR;

    #[test]
    fn a_write_moments_ago_is_still_the_same_boot() {
        let stamp = EcStamp::taken(500_000, 1_000_000);
        assert!(stamp.same_boot(500_002, 1_000_002));
    }

    #[test]
    fn an_ec_that_has_run_the_elapsed_time_is_still_the_same_boot() {
        let stamp = EcStamp::taken(500_000, 1_000_000);
        assert!(stamp.same_boot(500_000 + DAY, 1_000_000 + DAY));
    }

    #[test]
    fn an_ec_that_restarted_is_a_different_boot() {
        let stamp = EcStamp::taken(500_000, 1_000_000);
        assert!(!stamp.same_boot(60, 1_000_000 + HOUR));
    }

    #[test]
    fn clock_drift_over_a_long_uptime_is_not_a_restart() {
        let stamp = EcStamp::taken(0, 1_000_000);
        let elapsed = 10 * DAY;
        let one_percent_short = elapsed - elapsed / 100;
        assert!(stamp.same_boot(one_percent_short, 1_000_000 + elapsed));
    }

    #[test]
    fn an_ec_that_will_not_answer_withdraws_every_stamp() {
        let stamp = EcStamp::taken(500_000, 1_000_000);
        assert!(stamp.still_current(Some((500_002, 1_000_002))));
        assert!(!stamp.still_current(None));
    }

    #[test]
    fn a_recent_write_gets_the_floor_not_the_percentage() {
        let stamp = EcStamp::taken(10, 1_000_000);
        let within_the_floor = 1_000_000 + 30;
        let past_the_floor = 1_000_000 + 200;
        assert!(stamp.same_boot(10, within_the_floor));
        assert!(!stamp.same_boot(10, past_the_floor));
    }

    fn host_at(boot: &str, suspended_ms: u64) -> HostStamp {
        HostStamp {
            boot: boot.to_owned(),
            suspended_ms,
        }
    }

    fn host(suspended_ms: u64) -> HostStamp {
        host_at(BOOT, suspended_ms)
    }

    #[test]
    fn a_stamp_naming_this_boot_with_no_sleep_since_still_stands() {
        assert!(host(5_000).matches(Some(BOOT), Some(5_000)));
    }

    #[test]
    fn a_sleep_since_the_write_withdraws_a_stamp_from_this_boot() {
        assert!(!host(5_000).matches(Some(BOOT), Some(65_000)));
    }

    #[test]
    fn jitter_below_the_floor_is_not_a_sleep() {
        assert!(host(5_000).matches(Some(BOOT), Some(5_000 + SUSPEND_FLOOR_MS - 1)));
        assert!(!host(5_000).matches(Some(BOOT), Some(5_000 + SUSPEND_FLOOR_MS)));
    }

    #[test]
    fn a_stamp_from_an_earlier_boot_does_not_stand() {
        assert!(!host_at(EARLIER, 5_000).matches(Some(BOOT), Some(0)));
    }

    #[test]
    fn a_host_that_will_not_answer_withdraws_every_stamp() {
        assert!(!host(5_000).matches(None, Some(5_000)));
        assert!(!host(5_000).matches(Some(BOOT), None));
    }

    #[test]
    fn a_stamp_survives_the_round_trip_through_a_state_file() {
        let ec = EcStamp::parse(&EcStamp::taken(500_000, 1_000_000).stored()).expect("well formed");
        assert!(ec.same_boot(500_000, 1_000_000));
        let host = HostStamp::parse(&host(5_000).stored()).expect("well formed");
        assert!(host.matches(Some(BOOT), Some(5_000)));
    }

    #[test]
    fn a_line_that_is_not_a_stamp_parses_as_none() {
        assert!(EcStamp::parse("500000").is_none());
        assert!(EcStamp::parse("500000:tomorrow").is_none());
        assert!(HostStamp::parse(BOOT).is_none());
    }
}
