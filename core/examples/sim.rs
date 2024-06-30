use interlog_core::*;
use rand::prelude::*;
use std::collections::HashMap;

const ONE_DAY_IN_MS: u64 = 1000 * 60 * 60 * 24;

mod config {
	use interlog_core::storage;

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
	pub const SOURCE_BATCHES_PER_ACTOR: Range = Range(100, 1000);
	pub const PAYLOAD_SIZE: Range = Range(0, 4096);
	pub const EVENTS_PER_MSG: Range = Range(0, 50);

	// Currently just something "big enough", later handle disk overflow
	pub const DISK_CAPACITY: storage::Qty = storage::Qty(1000);
}

const TXN_SIZE: storage::Qty = storage::Qty(10);
const TXN_EVENTS_PER_ADDR: LogicalQty = LogicalQty(10);
const ACTUAL_EVENTS_PER_ADDR: LogicalQty = LogicalQty(10);

// Currently a paper thin wrapper around Buf.
// TODO: introduce faults
struct AppendOnlyMemory(event::Buf);

impl AppendOnlyMemory {
	fn new() -> Self {
		Self(event::Buf::new(config::DISK_CAPACITY))
	}
}

impl storage::AppendOnly for AppendOnlyMemory {
	fn used(&self) -> storage::Qty {
		self.0.used()
	}

	fn write(&mut self, data: &[u8]) -> Result<(), storage::WriteErr> {
		panic!("TODO");
	}
}

// An environment, representing some source of messages, and an actor
struct Env {
	actor: Actor<AppendOnlyMemory>,
	msg_batch_lens: Vec<usize>,
	payload_sizes: Vec<usize>,
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

		let batches_per_actor = config::SOURCE_BATCHES_PER_ACTOR.gen(rng);

		let msg_batch_lens: Vec<usize> =
			std::iter::repeat_with(|| config::EVENTS_PER_MSG.gen(rng))
				.take(batches_per_actor)
				.collect();

		let total_msgs = msg_batch_lens.iter().sum();

		let payload_sizes: Vec<usize> =
			std::iter::repeat_with(|| config::PAYLOAD_SIZE.gen(rng))
				.take(total_msgs)
				.collect();

		Self { actor, msg_batch_lens, payload_sizes }
	}
}

fn main() {
	let mut rng = thread_rng();

	let args: Vec<String> = std::env::args().collect();
	let seed: u64 = args
		.get(1)
		.map(|s| s.parse::<u64>().expect("a valid u64 seed"))
		.unwrap_or_else(|| rng.gen());

	let n_actors = config::ACTORS.gen(&mut rng);

	let mut environments: HashMap<Addr, Env> =
		std::iter::repeat_with(|| Env::new(&mut rng))
			.map(|env| (env.actor.addr, env))
			.take(n_actors)
			.collect();

	println!("Seed is {}", seed);
	println!("Number of actors {}", environments.len());

	let mut payload_buf = [0u8; config::PAYLOAD_SIZE.max()];
	let mut event_buf = event::Buf::new(TXN_SIZE);

	for tick in (0..ONE_DAY_IN_MS).step_by(10) {
		for env in environments.values_mut() {
			// receive messages from environment

			let payloads = env.msg_batch_lens.pop().map(|batch_len| {
				std::iter::repeat_with(move || {
					env.payload_sizes.pop().map(|payload_size| {
						let payload = &mut payload_buf[0..payload_size];
						rng.fill(payload);
						&payload_buf[0..payload_size]
					})
				})
				.take(batch_len)
				.flatten()
			});

			if let Some(payloads) = payloads {
				env.actor.local_write(payloads).expect("LOL HANDLE THIS")
			}

			/*
			if let Some(batch_len) = env.msg_batch_lens.pop() {
				for _ in 0..batch_len {
					if let Some(payload_size) = env.payload_sizes.pop() {
						let payload = &mut payload_buf[0..payload_size];
						rng.fill(payload);
						// TODO: need Illife vector of payloads, write that to actor
					}
				}
			}
			*/
		}
	}
}
