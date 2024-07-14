use interlog_core::*;
use rand::prelude::*;

struct AppendOnlyMemory(fixcap::Vec<u8>);

impl AppendOnlyMemory {
	fn new(capacity: usize) -> Self {
		Self(fixcap::Vec::new(capacity))
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

fn main() {
	let mut rng = SmallRng::from_entropy();
	let mut actor = Actor::new(
		Addr::new(&mut rng),
		Config {
			max_events: LogicalQty(2),
			txn_size: storage::Qty(4096),
			max_txn_events: LogicalQty(2),
		},
		AppendOnlyMemory::new(64),
	);

	let mut read_buf = event::Buf::new(storage::Qty(128));

	actor.enqueue(b"I have known the arcane law").unwrap();
	actor.commit().unwrap();
	actor.read(&mut read_buf, 0..=0);
	let actual = &read_buf.into_iter().last().unwrap();
	assert_eq!(actual.payload, b"I have known the arcane law");

	actor.enqueue(b"On strange roads, such visions met").unwrap();
	actor.commit().unwrap();
	actor.read(&mut read_buf, 0..=0);
	let actual = &read_buf.into_iter().last().unwrap();
	assert_eq!(actual.payload, b"On strange roads, such visions met");
}