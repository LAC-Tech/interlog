#[cfg(test)]
use proptest::prelude::*;

// TODO: too many allocations. Make a liffe vector implementation
#[cfg(test)]
pub fn arb_byte_list(max: usize) -> impl Strategy<Value = Vec<Vec<u8>>> {
    proptest::collection::vec(
        proptest::collection::vec(any::<u8>(), 0..=max),
        0..=max)
}

