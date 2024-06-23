use interlog_core::*;
use rand::prelude::*;
use std::collections::HashMap;

const ONE_DAY_IN_MS: u64 = 1000 * 60 * 60 * 24;

mod rand_num {
	pub struct Range(usize, usize);

	impl Range {
		pub fn gen<R: rand::Rng>(&self, rng: &mut R) -> usize {
			rng.gen_range(self.0..self.1)
		}

		pub const fn max(&self) -> usize {
			self.1
		}
	}

	const fn max(n: usize) -> Range {
		Range(0, n)
	}

	pub const ACTORS: Range = max(256);
	pub const LOCAL_EVENTS_PER_ACTOR: Range = Range(100, 1000);
	pub const PAYLOAD_SIZE: Range = Range(0, 4096);
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

// An environment, representing some source of messages, and an actor
struct Env {
	msg_src: Vec<usize>,
	actor: Actor<AppendOnlyMemory>,
}

impl Env {
	fn new<R: rand::Rng>(rng: &mut R) -> Self {
		let actor = Actor::new(
			Addr::new(rng),
			TXN_SIZE,
			TXN_EVENTS_PER_ADDR,
			ACTUAL_EVENTS_PER_ADDR,
			AppendOnlyMemory::new(),
		);

		let msg_src_len = rand_num::LOCAL_EVENTS_PER_ACTOR.gen(rng);

		let msg_src: Vec<usize> =
			std::iter::repeat_with(|| rand_num::PAYLOAD_SIZE.gen(rng))
				.take(msg_src_len)
				.collect();

		Self { msg_src, actor }
	}
}

fn main() {
	let mut rng = thread_rng();

	let args: Vec<String> = std::env::args().collect();
	let seed: u64 = args
		.get(1)
		.map(|s| s.parse::<u64>().expect("a valid u64 seed"))
		.unwrap_or_else(|| rng.gen());

	let n_actors = rand_num::ACTORS.gen(&mut rng);

	let mut actors: HashMap<Addr, Env> =
		std::iter::repeat_with(|| Env::new(&mut rng))
			.map(|env| (env.actor.addr, env))
			.take(n_actors)
			.collect();

	let mut payload_buf = [0u8; rand_num::PAYLOAD_SIZE.max()];

	println!("Seed is {}", seed);
	println!("Number of actors {}", actors.len());

	for tick in (0..ONE_DAY_IN_MS).step_by(10) {
		for env in actors.values_mut() {
			if let Some(msg_size) = env.msg_src.pop() {
				rng.fill(&mut payload_buf[0..msg_size]);
			}
		}
	}
}
