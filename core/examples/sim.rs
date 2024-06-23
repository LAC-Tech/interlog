use hashbrown::HashMap;
use interlog_core::*;
use rand::prelude::*;

const TXN_EVENTS_PER_ADDR: LogicalQty = LogicalQty(10);
const ACTUAL_EVENTS_PER_ADDR: LogicalQty = LogicalQty(10);

fn main() {
	let mut rng = thread_rng();
	/*
	std::iter::repeat_with(|| Addr::new([rng.gen(), rng.gen()])).map(|addr| {
		(addr, Actor::new(addr, TXN_EVENTS_PER_ADDR, ACTUAL_EVENTS_PER_ADDR))
	});
	*/
}
