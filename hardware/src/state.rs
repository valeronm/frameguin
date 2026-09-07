//! The store for what cannot be read back, and for what was asked for.
//!
//! The haptic touchpad ACKs `GET_FEATURE` with zeros, the charge current
//! limit has no readback in any command version, and the touch panel's own
//! enable command asks for no reply, so what was written is only knowable
//! from a mirror. One file, keyed: a mirror is two lines, `<key>` for the
//! value and `<key>_evidence` for what proves the holder still has it, a
//! wanted value one line under its own prefix, read and written through
//! [`Store`], so a key another version wrote is carried across a save
//! rather than dropped, and a key this version does not know costs nothing
//! but the line.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::Mutex;

const STATE_FILE: &str = "/var/lib/frameguin/state";

/// Where a device keeps what it cannot read back and what it was asked for.
/// A `None` value removes the key, for a mirror whose presence is the whole
/// of its claim.
pub trait Store: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: Option<String>);
    /// The wanted values are dropped as a set, so a device absent this run
    /// loses its record with the rest.
    fn drop_prefix(&self, prefix: &str);
}

/// A value the store can keep and name again. What the store cannot name
/// is refused here, so nothing holds it.
pub trait Stored: Clone + Send {
    fn from_stored(value: &str) -> Option<Self>;
    fn stored(&self) -> String;

    fn load(store: &dyn Store, key: &str) -> Option<Self> {
        store.get(key).and_then(|v| Self::from_stored(&v))
    }
}

macro_rules! stored_by_parsing {
    ($($t:ty),*) => {$(
        impl Stored for $t {
            fn from_stored(value: &str) -> Option<Self> {
                value.parse().ok()
            }

            fn stored(&self) -> String {
                self.to_string()
            }
        }
    )*};
}

stored_by_parsing!(NonZeroU32, bool, u8);

/// The state file, held whole and written whole on every change.
pub struct StateFile {
    entries: Mutex<BTreeMap<String, String>>,
}

impl StateFile {
    /// A missing or unreadable file is an empty store: every mirror then
    /// answers its default until the first write, which on a machine whose
    /// touchpad was already changed by other means is a misreport nothing
    /// can avoid, the hardware being unreadable.
    pub fn load() -> Self {
        let entries = std::fs::read_to_string(STATE_FILE)
            .map(|content| parse(&content))
            .unwrap_or_default();
        Self {
            entries: Mutex::new(entries),
        }
    }
}

fn parse(content: &str) -> BTreeMap<String, String> {
    content
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.trim().to_owned()))
        .collect()
}

/// What a `set` means to the map, spelled once for every store over one;
/// answers whether the map moved.
pub(crate) fn apply(
    entries: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<String>,
) -> bool {
    match (entries.get_mut(key), value) {
        (Some(held), Some(value)) if *held == value => false,
        (Some(held), Some(value)) => {
            *held = value;
            true
        }
        (None, Some(value)) => {
            entries.insert(key.to_owned(), value);
            true
        }
        (_, None) => entries.remove(key).is_some(),
    }
}

/// Answers whether the map moved, as [`apply`] does.
pub(crate) fn drop_prefix(entries: &mut BTreeMap<String, String>, prefix: &str) -> bool {
    let before = entries.len();
    entries.retain(|key, _| !key.starts_with(prefix));
    entries.len() != before
}

fn render(entries: &BTreeMap<String, String>) -> String {
    use std::fmt::Write;
    entries.iter().fold(String::new(), |mut out, (key, value)| {
        let _ = writeln!(out, "{key}={value}");
        out
    })
}

impl Store for StateFile {
    fn get(&self, key: &str) -> Option<String> {
        self.entries.lock().unwrap().get(key).cloned()
    }

    fn set(&self, key: &str, value: Option<String>) {
        self.change(|entries| apply(entries, key, value));
    }

    fn drop_prefix(&self, prefix: &str) {
        self.change(|entries| drop_prefix(entries, prefix));
    }
}

impl StateFile {
    // The directory is provisioned by StateDirectory= in the systemd unit.
    fn change(&self, change: impl FnOnce(&mut BTreeMap<String, String>) -> bool) {
        let content = {
            let mut entries = self.entries.lock().unwrap();
            if !change(&mut entries) {
                return;
            }
            render(&entries)
        };
        if let Err(e) = std::fs::write(STATE_FILE, content) {
            eprintln!("failed to persist state: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, render};

    #[test]
    fn a_file_round_trips_with_its_unknown_keys() {
        let content = "click_force=2\nsomething_newer=yes\n";
        assert_eq!(render(&parse(content)), content);
    }

    #[test]
    fn a_line_without_a_separator_is_skipped() {
        assert!(parse("garbage\n").is_empty());
    }
}
