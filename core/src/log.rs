use super::storage;
use crate::event;
use crate::mem;
use crate::pervasives::*;

pub struct Log<AOS: storage::AppendOnly> {
	txn_last: LogicalQty,
	actual_last: LogicalQty,
	txn_buf: event::Buf,
	storage: AOS,
}

impl<AOS: storage::AppendOnly> Log<AOS> {
	pub fn new(txn_size: storage::Qty, aos: AOS) -> Self {
		Self {
			txn_last: LogicalQty(0),
			actual_last: LogicalQty(0),
			txn_buf: event::Buf::new(txn_size),
			storage: aos,
		}
	}
	pub fn enqueue(
		&mut self,
		e: &event::Event,
	) -> Result<storage::Qty, mem::Overrun> {
		let result = self.storage.used() + self.txn_buf.used();
		self.txn_buf.push(e)?;
		self.txn_last += LogicalQty(1);
		Ok(result)
	}

	pub fn commit(&mut self) -> Result<usize, storage::Overrun> {
		self.storage.write(self.txn_buf.as_bytes())?;
		// TODO: ask storage directly for this? I think pwrite gives it
		let n_events_comitted = self.txn_last;
		self.actual_last += self.txn_last;
		self.txn_buf.clear();
		Ok(n_events_comitted.0)
	}

	pub fn rollback(&mut self) {
		self.txn_buf.clear();
	}

	pub fn next_pos(&self) -> LogicalQty {
		self.actual_last + self.txn_last
	}
}
