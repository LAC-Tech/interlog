#[cfg(test)]
use proptest::prelude::*;

// TODO: too many allocations. Make a liffe vector implementation
#[cfg(test)]
pub fn arb_local_events(
	outer_max: usize,
	inner_max: usize,
) -> impl Strategy<Value = Vec<Vec<u8>>> {
	proptest::collection::vec(
		proptest::collection::vec(any::<u8>(), 0..=inner_max),
		0..=outer_max,
	)
}

#[cfg(test)]
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
