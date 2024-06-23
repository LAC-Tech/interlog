use interlog_core::*;
use rand::prelude::*;
use std::collections::HashMap;

mod config {
	pub struct Max(usize);

	impl Max {
		pub fn gen<R: rand::Rng>(&self, rng: &mut R) -> usize {
			rng.gen_range(0..self.0)
		}
	}

	pub const N_ACTORS: Max = Max(256);
}

const TXN_SIZE: StorageQty = StorageQty(10);
const TXN_EVENTS_PER_ADDR: LogicalQty = LogicalQty(10);
const ACTUAL_EVENTS_PER_ADDR: LogicalQty = LogicalQty(10);

struct AppendOnlyMemory(Vec<mem::Word>);

impl AppendOnlyMemory {
	fn new() -> Self {
		Self(Vec::new())
	}
}

impl AppendOnlyStorage for AppendOnlyMemory {
	fn used(&self) -> StorageQty {
		StorageQty(self.0.len())
	}
}

fn main() {
	let mut rng = thread_rng();

	let args: Vec<String> = std::env::args().collect();
	let seed: u64 = args
		.get(1)
		.map(|s| s.parse::<u64>().expect(""))
		.unwrap_or_else(|| rng.gen());

	let n_actors = config::N_ACTORS.gen(&mut rng);

	let actors: HashMap<Addr, Actor<AppendOnlyMemory>> =
		std::iter::repeat_with(|| Addr::new(&mut rng))
			.map(|addr| {
				let aos = AppendOnlyMemory::new();

				let actor = Actor::new(
					addr,
					TXN_SIZE,
					TXN_EVENTS_PER_ADDR,
					ACTUAL_EVENTS_PER_ADDR,
					aos,
				);

				(addr, actor)
			})
			.take(n_actors)
			.collect();

	println!("Seed is {}", seed);
	println!("Number of actors {}", actors.len());
}
