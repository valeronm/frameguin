//! The store for what cannot be read back.
//!
//! The haptic touchpad ACKs `GET_FEATURE` with zeros, the charge current
//! limit has no readback in any command version, and the touch panel's own
//! enable command asks for no reply, so what was written is only knowable
//! from a mirror. One file, keyed: each device names its own keys and reads
//! and writes them through [`Store`], so a key another version wrote is
//! carried across a save rather than dropped, and a mirror this version does
//! not know costs nothing but the line.

use std::collections::BTreeMap;
use std::sync::Mutex;

const STATE_FILE: &str = "/var/lib/frameguin/state";

/// Where a device keeps what it cannot read back. A `None` value removes the
/// key, for a mirror whose presence is the whole of its claim.
pub trait Store: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: Option<String>);
}

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

/// What a `set` means to the map, spelled once for every store over one.
fn apply(entries: &mut BTreeMap<String, String>, key: &str, value: Option<String>) {
    match value {
        Some(value) => entries.insert(key.to_owned(), value),
        None => entries.remove(key),
    };
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

    // The directory is provisioned by StateDirectory= in the systemd unit.
    fn set(&self, key: &str, value: Option<String>) {
        let content = {
            let mut entries = self.entries.lock().unwrap();
            apply(&mut entries, key, value);
            render(&entries)
        };
        if let Err(e) = std::fs::write(STATE_FILE, content) {
            eprintln!("failed to persist state: {e}");
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::{Store, apply, parse, render};

    /// A store that never touches disk, for exercising a device's mirror.
    #[derive(Default)]
    pub struct Memory(Mutex<BTreeMap<String, String>>);

    impl Store for Memory {
        fn get(&self, key: &str) -> Option<String> {
            self.0.lock().unwrap().get(key).cloned()
        }

        fn set(&self, key: &str, value: Option<String>) {
            apply(&mut self.0.lock().unwrap(), key, value);
        }
    }

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
