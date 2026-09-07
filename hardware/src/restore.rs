//! What the user asked a control to be, and the switch that has it written
//! back: firmware re-asserts its own value for several controls at POST and
//! on a resume, and a choice made here is what outlives that.

use std::marker::PhantomData;
use std::sync::Arc;

use frameguin_wire::DeviceResult;

use crate::state::{Store, Stored};

const KEY_RESTORE: &str = "restore";
const WANTED_PREFIX: &str = "wanted_";

/// A device with something to write back.
pub trait Restorable {
    fn restore(&self) -> impl Future<Output = DeviceResult<()>> + Send;
}

/// The switch. On, every setter records what it was asked for, and a
/// restore writes it all back.
#[derive(Clone)]
pub struct Restore {
    store: Arc<dyn Store>,
}

impl Restore {
    pub(crate) fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    pub fn enabled(&self) -> bool {
        self.store.get(KEY_RESTORE).is_some()
    }

    /// Off drops every wanted value with it, so the switch going on again
    /// restores nothing chosen before that.
    pub fn set_enabled(&self, enabled: bool) {
        self.store.set(KEY_RESTORE, enabled.then(|| true.stored()));
        if !enabled {
            self.store.drop_prefix(WANTED_PREFIX);
        }
    }

    pub(crate) fn wanted<V: Stored>(self, name: &str) -> Wanted<V> {
        Wanted {
            key: format!("{WANTED_PREFIX}{name}"),
            restore: self,
            value: PhantomData,
        }
    }
}

/// What a control was last asked to be, under its own key.
pub struct Wanted<V> {
    restore: Restore,
    key: String,
    value: PhantomData<V>,
}

impl<V: Stored> Wanted<V> {
    /// Kept only while the switch is on: a choice made with it off is one
    /// nobody asked to have written back.
    pub fn record(&self, value: &V) {
        if self.restore.enabled() {
            self.restore.store.set(&self.key, Some(value.stored()));
        }
    }

    pub fn forget(&self) {
        self.restore.store.set(&self.key, None);
    }

    pub fn current(&self) -> Option<V> {
        V::load(self.restore.store.as_ref(), &self.key)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::Restore;
    use crate::state::Store;
    use crate::testing::{Memory, mirrors};

    fn switch(store: &Arc<Memory>) -> Restore {
        mirrors(store, None, None).restore()
    }

    #[test]
    fn a_value_is_recorded_only_while_the_switch_is_on() {
        let store = Arc::new(Memory::default());
        let restore = switch(&store);
        let wanted = restore.clone().wanted::<u8>("limit");
        wanted.record(&80);
        assert_eq!(wanted.current(), None);
        restore.set_enabled(true);
        wanted.record(&80);
        assert_eq!(wanted.current(), Some(80));
        assert_eq!(store.get("wanted_limit").as_deref(), Some("80"));
        wanted.forget();
        assert_eq!(wanted.current(), None);
    }

    #[test]
    fn switching_off_drops_every_wanted_value() {
        let store = Arc::new(Memory::default());
        let restore = switch(&store);
        restore.set_enabled(true);
        assert!(restore.enabled());
        restore.clone().wanted::<u8>("limit").record(&80);
        restore.clone().wanted::<bool>("off").record(&true);
        store.set("charge_current_limit", Some("1500".into()));
        restore.set_enabled(false);
        assert!(!restore.enabled());
        assert_eq!(store.get("wanted_limit"), None);
        assert_eq!(store.get("wanted_off"), None);
        assert_eq!(store.get("charge_current_limit").as_deref(), Some("1500"));
    }
}
