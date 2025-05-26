//! Rust's std HashMap is a major source of non-determinism.
//! Cobbling the two libraries to take deterministic ones is a bit fiddly
//! Putting it here so I don't have to look at it all the time

use foldhash::fast::FixedState;
pub use hashbrown::hash_map::Entry;

pub type HashMap<K, V> = hashbrown::HashMap<K, V, FixedState>;

pub trait Ext {
    fn new(seed: u64) -> Self;
}

impl<K, V> Ext for HashMap<K, V> {
    fn new(seed: u64) -> Self {
        HashMap::with_hasher(FixedState::with_seed(seed))
    }
}
