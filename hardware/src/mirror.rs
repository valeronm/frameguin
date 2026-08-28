//! A mirror: the last write the hardware took, for a value it cannot read
//! back, believed for the lifetime of whatever holds it.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use frameguin_wire::DeviceResult;

use crate::lifetime::{Evidence, Holders, Lifetime};
use crate::state::Store;

/// Where a device's mirrors are cut from.
pub struct Mirrors {
    store: Arc<dyn Store>,
    holders: Holders,
}

impl Mirrors {
    pub fn new(store: Arc<dyn Store>, holders: Holders) -> Self {
        Self { store, holders }
    }

    /// A mirror holding a value under `key`, believed for `lifetime`. One
    /// mirror per key: a second over the same key holds a copy the first's
    /// writes do not move.
    pub fn value<V: Stored>(&self, key: &str, lifetime: Lifetime) -> Mirror<V> {
        let value = self.store.get(key).and_then(|v| V::from_stored(&v));
        let evidence = lifetime.recall(self.store.get(&evidence_key(key)).as_deref());
        Mirror {
            store: self.store.clone(),
            holders: self.holders.clone(),
            key: key.to_owned(),
            lifetime,
            held: Mutex::new(value.zip(evidence)),
        }
    }
}

/// Where a mirror keeps the evidence for the value under `key`.
pub(crate) fn evidence_key(key: &str) -> String {
    format!("{key}_evidence")
}

/// A value a mirror can keep in the store and name again. What the store
/// cannot name is refused here, so a mirror never holds it.
pub trait Stored: Clone + Send {
    fn from_stored(value: &str) -> Option<Self>;
    fn stored(&self) -> String;
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

stored_by_parsing!(NonZeroU32, bool);

pub struct Mirror<V> {
    store: Arc<dyn Store>,
    holders: Holders,
    key: String,
    lifetime: Lifetime,
    held: Mutex<Option<(V, Evidence)>>,
}

impl<V: Stored> Mirror<V> {
    /// Makes the write and remembers `value` once the hardware has taken it.
    /// Evidence is witnessed before the write, so a holder's life ending
    /// between the two withdraws the record rather than vouching for it.
    pub fn record(&self, value: V, write: impl FnOnce() -> DeviceResult<()>) -> DeviceResult<()> {
        let evidence = self.lifetime.witness(&self.holders);
        write()?;
        self.hold(evidence.map(|evidence| (value, evidence)));
        Ok(())
    }

    /// Makes the write and drops the record, for a write that returns the
    /// value to the state the hardware comes up in.
    pub fn clear(&self, write: impl FnOnce() -> DeviceResult<()>) -> DeviceResult<()> {
        write()?;
        self.hold(None);
        Ok(())
    }

    pub fn current(&self) -> Option<V> {
        let held = self.held.lock().unwrap();
        held.as_ref()
            .filter(|(_, evidence)| evidence.proves(&self.holders))
            .map(|(value, _)| value.clone())
    }

    fn hold(&self, held: Option<(V, Evidence)>) {
        self.store
            .set(&self.key, held.as_ref().map(|(value, _)| value.stored()));
        self.store.set(
            &evidence_key(&self.key),
            held.as_ref().and_then(|(_, evidence)| evidence.stored()),
        );
        *self.held.lock().unwrap() = held;
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::sync::Arc;

    use super::evidence_key;
    use crate::lifetime::{EcBoot, Lifetime};
    use crate::state::Store;
    use crate::state::tests::Memory;
    use crate::testing::mirrors;

    const THREE: NonZeroU32 = NonZeroU32::new(3).unwrap();

    #[test]
    fn a_permanent_value_keeps_no_evidence() {
        let store = Arc::new(Memory::default());
        let mirror = mirrors(&store, None, None).value::<bool>("held", Lifetime::Permanent);
        mirror.record(true, || Ok(())).unwrap();
        assert_eq!(mirror.current(), Some(true));
        assert_eq!(store.get("held").as_deref(), Some("true"));
        assert_eq!(store.get(&evidence_key("held")), None);
    }

    #[test]
    fn a_value_of_the_ecs_lifetime_outlives_a_reload_and_not_a_restart() {
        let store = Arc::new(Memory::default());
        let written = EcBoot::from_clocks(500_000, 1_000_000);
        mirrors(&store, Some(written), None)
            .value::<NonZeroU32>("cap", Lifetime::Ec)
            .record(THREE, || Ok(()))
            .unwrap();
        let same_boot = EcBoot::from_clocks(500_010, 1_000_010);
        let reloaded =
            mirrors(&store, Some(same_boot), None).value::<NonZeroU32>("cap", Lifetime::Ec);
        assert_eq!(reloaded.current(), Some(THREE));
        let restarted = EcBoot::from_clocks(10, 1_003_600);
        let reloaded =
            mirrors(&store, Some(restarted), None).value::<NonZeroU32>("cap", Lifetime::Ec);
        assert_eq!(reloaded.current(), None);
    }
}
