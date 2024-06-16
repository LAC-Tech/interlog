//! Useful random generators.
use alloc::vec::Vec;
use proptest::prelude::*;

use crate::event;
use crate::pervasives::*;

// TODO: too many allocations. Make a liffe vector implementation
pub fn arb_local_events(
	outer_max: usize,
	inner_max: usize,
) -> impl Strategy<Value = Vec<Vec<u8>>> {
	proptest::collection::vec(
		proptest::collection::vec(any::<u8>(), 0..=inner_max),
		0..=outer_max,
	)
}

pub fn arb_local_events_stream(
	stream_max: usize,
	outer_max: usize,
	inner_max: usize,
) -> impl Strategy<Value = Vec<Vec<Vec<u8>>>> {
	proptest::collection::vec(
		arb_local_events(outer_max, inner_max),
		0..=stream_max,
	)
}

pub fn arb_addr() -> impl Strategy<Value = Addr> {
	(any::<u64>(), any::<u64>()).prop_map(|(a, b)| Addr::new([a, b]))
}

pub fn arb_log_pos() -> impl Strategy<Value = LogicalPos> {
	(0usize..1000usize).prop_map(LogicalPos)
}

pub fn arb_storage_pos() -> impl Strategy<Value = StoragePos> {
	(0usize..1000usize).prop_map(StoragePos)
}

pub fn arb_event_id() -> impl Strategy<Value = event::ID> {
	(arb_addr(), arb_log_pos())
		.prop_map(|(addr, log_pos)| event::ID::new(addr, log_pos))
}

pub fn addresses<R: Rng, const LEN: usize>(rng: &mut R) -> [Addr; LEN] {
	core::array::from_fn(|_| Addr::new(rng.gen()))
}
