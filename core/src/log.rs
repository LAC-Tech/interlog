use super::storage;
use crate::event;
use crate::mem;
use crate::pervasives::*;

struct Txn {
	last: LogicalQty,
	buf: event::Buf,
}

impl Txn {
	fn new(size: storage::Qty) -> Self {
		Self { last: LogicalQty(0), buf: event::Buf::new(size) }
	}

	fn clear(&mut self) {
		self.last = LogicalQty(0);
		self.buf.clear();
	}

	fn push(&mut self, e: &event::Event) -> Result<(), mem::Overrun> {
		self.buf.push(e).map(|()| self.last += LogicalQty(1))
	}

	fn used(&self) -> storage::Qty {
		self.buf.used()
	}
}

pub struct Log<AOS: storage::AppendOnly> {
	txn: Txn,
	actual_last: LogicalQty,
	storage: AOS,
}

impl<AOS: storage::AppendOnly> Log<AOS> {
	pub fn new(txn_size: storage::Qty, aos: AOS) -> Self {
		Self {
			actual_last: LogicalQty(0),
			txn: Txn::new(txn_size),
			storage: aos,
		}
	}
	pub fn enqueue(
		&mut self,
		e: &event::Event,
	) -> Result<storage::Qty, mem::Overrun> {
		let result = self.storage.used() + self.txn.used();
		self.txn.push(e)?;
		Ok(result)
	}

	pub fn commit(&mut self) -> Result<usize, storage::Overrun> {
		self.storage.append(self.txn.buf.as_bytes())?;
		// TODO: ask storage directly for this? I think pwrite gives it
		let n_events_comitted = self.txn.last;
		self.actual_last += n_events_comitted;
		self.txn.clear();
		Ok(n_events_comitted.0)
	}

	pub fn rollback(&mut self) {
		self.txn.clear();
	}

	pub fn next_pos(&self) -> LogicalQty {
		self.actual_last + self.txn.last
	}
}
