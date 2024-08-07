use interlog_core::*;
use rand::prelude::*;
use std::collections::BTreeMap;
use std::iter;
use std::panic::{self, AssertUnwindSafe};

const MAX_SIM_TIME_MS: u64 = 1000 * 60 * 60; // One hour

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

	pub struct Bool(usize);

	impl Bool {
		pub fn gen<R: rand::Rng>(&self, rng: &mut R) -> bool {
			rng.gen_range(0..100) < self.0
		}
	}

	pub const N_LOGS: Range = max(256);
	pub const PAYLOADS_PER_LOG: Range = Range(100, 1000);
	pub const PAYLOAD_SIZE: Range = Range(0, 4096);
	pub const MSG_LEN: Range = Range(0, 50);
	pub const CHANCE_OF_WRITE_PER_TICK: Range = Range(1, 50);

	// Currently just something "big enough", later handle disk overflow
	pub const STORAGE_CAPACITY: storage::Qty = storage::Qty(10_000_000);
}

// TODO: introduce faults
struct AppendOnlyMemory(fixcap::Vec<u8>);

impl AppendOnlyMemory {
	fn new() -> Self {
		Self(fixcap::Vec::new(config::STORAGE_CAPACITY.0))
	}
}

impl storage::AppendOnly for AppendOnlyMemory {
	fn used(&self) -> storage::Qty {
		storage::Qty(self.0.len())
	}

	fn append(&mut self, data: &[u8]) -> Result<(), storage::Overrun> {
		self.0
			.extend_from_slice(data)
			// TODO: should storage overrun have the same fields?
			.map_err(|mem::Overrun { .. }| storage::Overrun)
	}

	fn read(&self, buf: &mut [u8], offset: usize) {
		buf.copy_from_slice(&self.0[offset..offset + buf.len()])
	}
}

// An environment, representing some source of messages, and a log
struct Env {
	log: Log<AppendOnlyMemory>,
	// The dimensions of each of the messages sent are pre-calculated
	msg_lens: Vec<usize>,
	payload_sizes: Vec<usize>,
}

impl Env {
	fn new<R: rand::Rng>(rng: &mut R) -> Self {
		let config = ExtAllocMem {
			txn_size: storage::Qty(
				config::MSG_LEN.max() * config::PAYLOAD_SIZE.max(),
			),
			max_txn_events: LogicalQty(config::MSG_LEN.max()),
			max_events: LogicalQty(1_000_000),
		};

		let log = Log::new(Addr::new(rng), config, AppendOnlyMemory::new());
		let payloads_per_actor = config::PAYLOADS_PER_LOG.gen(rng);

		let msg_lens: Vec<usize> =
			iter::repeat_with(|| config::MSG_LEN.gen(rng))
				.take(payloads_per_actor)
				.collect();

		let total_msgs = msg_lens.iter().sum();

		let payload_sizes: Vec<usize> =
			iter::repeat_with(|| config::PAYLOAD_SIZE.gen(rng))
				.take(total_msgs)
				.collect();

		Self { log, msg_lens, payload_sizes }
	}

	fn payload_lens(
		&mut self,
		msg_len: usize,
	) -> impl Iterator<Item = usize> + '_ {
		(0..msg_len).map(|_| {
			self.payload_sizes.pop().expect("enough sizes for each msg len")
		})
	}

	/// returns true if it will produce new data next tick
	pub fn tick<R: rand::Rng, const N: usize>(
		&mut self,
		ms: u64,
		rng: &mut R,
		ctx: &mut Context<N>,
	) -> bool {
		match self.msg_lens.pop() {
			None => {
				assert_eq!(
					self.payload_sizes.len(),
					0,
					"zero message lens means zero payload sizes"
				);
				false
			}
			Some(msg_len) => {
				ctx.payload_lens.clear();
				ctx.payload_lens
					.extend(self.payload_lens(msg_len))
					.expect("payload lens to be big enough");

				for &len in &ctx.payload_lens {
					let payload = &mut ctx.payload_buf[..len];
					rng.fill(payload);
					self.log
						.enqueue(payload)
						.expect("log to have enough txn buffer");
				}

				let events_committed = self
					.log
					.commit()
					.expect("log to have enough space to persist all events");

				ctx.stats.update(events_committed);
				true
			}
		}
	}
}

struct Context<const N: usize> {
	stats: Stats,
	payload_buf: [u8; N],
	payload_lens: fixcap::Vec<usize>,
}

#[derive(Debug)]
struct Stats {
	total_events_committed: usize,
	total_commits: usize,
}

impl Stats {
	fn update(&mut self, events_committed: usize) {
		self.total_events_committed = self
			.total_events_committed
			.checked_add(events_committed)
			.expect("no overflow");

		self.total_commits =
			self.total_commits.checked_add(1).expect("no overflow");
	}
}

fn main() {
	let args: Vec<String> = std::env::args().collect();
	let seed: u64 = args
		.get(1)
		.map(|s| s.parse::<u64>().expect("a valid u64 seed"))
		.unwrap_or_else(|| thread_rng().gen());

	let mut rng = SmallRng::seed_from_u64(seed);

	let n_logs = config::N_LOGS.gen(&mut rng);

	let mut environments: BTreeMap<Addr, Env> =
		std::iter::repeat_with(|| Env::new(&mut rng))
			.map(|env| (env.log.addr, env))
			.take(n_logs)
			.collect();

	println!("Seed is {}", seed);
	println!("Number of actors {}", environments.len());

	let mut ctx = Context {
		stats: Stats { total_events_committed: 0, total_commits: 0 },
		payload_buf: [0u8; config::PAYLOAD_SIZE.max()],
		payload_lens: fixcap::Vec::new(config::MSG_LEN.max()),
	};

	let mut dead_addrs: Vec<Addr> = vec![];

	for ms in (0..MAX_SIM_TIME_MS).step_by(10) {
		for addr in &dead_addrs {
			environments.remove(&addr);
		}

		dead_addrs.clear();

		for env in environments.values_mut() {
			let write_res = panic::catch_unwind(AssertUnwindSafe(|| {
				let more_data = env.tick(ms, &mut rng, &mut ctx);

				if !more_data {
					dead_addrs.push(env.log.addr.clone());
				}
			}));

			if let Err(err) = write_res {
				println!(
					"the time is {:?} ms, do you know where your data is?",
					ms
				);
				println!("Error for {:?} at {:?}ms", env.log.addr, ms);
				println!("{}", err.downcast_ref::<String>().unwrap());
				return;
			}
		}
	}

	println!("{:?}", ctx.stats);
	println!(
		"{:?} transactions/second",
		ctx.stats.total_commits as f64 / (MAX_SIM_TIME_MS as f64 / 1000.0),
	);
}
