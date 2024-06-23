use std::collections::HashMap;
use interlog_core::*;
use rand::prelude::*;

mod config {
	struct Max(usize);

	impl Max {
		fn gen<R: rand::Rng>(&self, rng: &mut R) -> usize {
			rng.gen_range(0..self.0)
		}
	}
	pub const N_ACTORS: Max = Max(256);
}

const TXN_EVENTS_PER_ADDR: LogicalQty = LogicalQty(10);
const ACTUAL_EVENTS_PER_ADDR: LogicalQty = LogicalQty(10);

fn main() {
	let mut rng = thread_rng();

	let mut rng = rand::thread_rng();
	let args: Vec<String> = std::env::args().collect();
	let seed: u64 = args.get(1)
		.map(|s| s.parse::<u64>().expect(""))
		.unwrap_or_else(|| rng.gen());
	/*
	std::iter::repeat_with(|| Addr::new([rng.gen(), rng.gen()])).map(|addr| {
		(addr, Actor::new(addr, TXN_EVENTS_PER_ADDR, ACTUAL_EVENTS_PER_ADDR))
	});
	*/
}
