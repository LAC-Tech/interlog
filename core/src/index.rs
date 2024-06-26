use crate::event;
use crate::fixed_capacity;
use crate::fixed_capacity::Vec;
use crate::pervasives::*;
use crate::storage;

use hashbrown::HashMap;

#[derive(Clone, Debug, PartialEq)]
struct Elem {
	txn: Vec<storage::Qty>,
	actual: Vec<storage::Qty>,
}

impl Elem {
	fn is_empty(&self) -> bool {
		self.actual.is_empty()
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct Index {
	// TODO: fixed size hashmap
	map: HashMap<Addr, Elem>,
	txn_events_per_addr: LogicalQty,
	actual_events_per_addr: LogicalQty,
}

/// In-memory mapping of event IDs to disk offsets
/// This keeps the following invariants:
/// - events must be stored consecutively per address,
///
/// This effectively stores the causal histories over every addr
impl Index {
	pub fn new(
		txn_events_per_addr: LogicalQty,
		actual_events_per_addr: LogicalQty,
	) -> Self {
		// TODO: HOW MANY ADDRS WILL I HAVE?
		Self {
			map: HashMap::new(),
			txn_events_per_addr,
			actual_events_per_addr,
		}
	}

	pub fn insert<EID: Into<event::ID>>(
		&mut self,
		event_id: EID,
		offset: storage::Qty,
	) -> Result<(), InsertErr> {
		let event_id: event::ID = event_id.into();
		match self.map.get_mut(&event_id.origin) {
			Some(existing) => {
				if existing.txn.len() != event_id.pos.0 {
					return Err(InsertErr::NonConsecutivePos);
				}

				if let Some(&last_offset) = existing.txn.last() {
					if last_offset >= offset {
						panic!("non-monotonic storage offset")
					}
				}

				if let Err(fixed_capacity::Overrun) = existing.txn.push(offset)
				{
					return Err(InsertErr::Overrun);
				}

				Ok(())
			}
			None => {
				if !event_id.pos.is_initial() {
					return Err(InsertErr::IndexWouldNotStartAtZero);
				}

				let mut txn = Vec::new(self.txn_events_per_addr.0);
				if let Err(fixed_capacity::Overrun) = txn.push(offset) {
					return Err(InsertErr::Overrun);
				}

				let actual = Vec::new(self.actual_events_per_addr.0);

				self.map.insert(event_id.origin, Elem { txn, actual });
				Ok(())
			}
		}
	}

	pub fn commit(&mut self) -> Result<(), CommitErr> {
		let enough_space = self
			.map
			.values()
			.all(|Elem { txn, actual }| actual.can_be_extended_by(txn));

		if !enough_space {
			return Err(CommitErr::NotEnoughSpace);
		}

		for Elem { txn, actual } in self.map.values_mut() {
			if let Err(fixed_capacity::Overrun) = actual.extend_from_slice(txn)
			{
				return Err(CommitErr::Overrun);
			}
			txn.clear();
		}

		Ok(())
	}

	pub fn rollback(&mut self) {
		// remove empty elems left behind by a failed transaction
		self.map.retain(|_, v| !v.is_empty());
		// remove items from txn buffers
		for Elem { txn, .. } in self.map.values_mut() {
			txn.clear()
		}
	}

	pub fn get(&self, event_id: event::ID) -> Option<storage::Qty> {
		self.map
			.get(&event_id.origin)
			.and_then(|elem| elem.actual.get(event_id.pos.0))
			.cloned()
	}

	pub fn event_count(&self) -> usize {
		self.map.values().map(|elem| elem.actual.len()).sum()
	}
}

#[derive(Debug, PartialEq)]
pub enum InsertErr {
	NonConsecutivePos,
	IndexWouldNotStartAtZero,
	Overrun,
}

#[derive(Debug, PartialEq)]
pub enum CommitErr {
	NotEnoughSpace,
	Overrun,
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::test_utils::*;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;
	use rand::prelude::*;

	// Sanity check unit tests - trigger every error

	#[test]
	fn non_consecutive() {
		let mut rng = thread_rng();
		let addr = Addr::new(rng.gen());
		let mut index = Index::new(LogicalQty(2), LogicalQty(8));

		let actual = index.insert((addr, LogicalQty(0)), StorageQty(0));
		assert_eq!(actual, Ok(()));

		let actual = index.insert((addr, LogicalQty(34)), StorageQty(0));
		assert_eq!(actual, Err(InsertErr::NonConsecutivePos));
	}

	#[test]
	fn index_would_not_start_at_zero() {
		let mut rng = thread_rng();
		let addr = Addr::new(rng.gen());
		let mut index = Index::new(LogicalQty(2), LogicalQty(8));

		let actual = index.insert((addr, LogicalQty(42)), StorageQty(0));
		assert_eq!(actual, Err(InsertErr::IndexWouldNotStartAtZero));
	}

	#[test]
	fn overlow() {
		let mut rng = thread_rng();
		let addr = Addr::new(rng.gen());
		let mut index = Index::new(1, 8);

		let actual = index.insert((addr, LogicalQty(0)), StorageQty(0));
		assert_eq!(actual, Ok(()));

		let actual = index.insert((addr, LogicalQty(1)), StorageQty(1));
		assert_eq!(actual, Err(InsertErr::Overrun));
	}

	#[test]
	fn not_enough_space() {
		let mut rng = thread_rng();
		let addr = Addr::new(rng.gen());
		let mut index = Index::new(LogicalQty(2), LogicalQty(1));

		let actual = index.insert((addr, LogicalQty(0)), StorageQty(0));
		assert_eq!(actual, Ok(()));

		let actual = index.insert((addr, LogicalQty(1)), StorageQty(1));
		assert_eq!(actual, Ok(()));

		let actual = index.commit();
		assert_eq!(actual, Err(CommitErr::NotEnoughSpace))
	}

	#[test]
	fn enqueue_and_get() {
		let mut rng = thread_rng();
		let addr = Addr::new(rng.gen());
		let mut index = Index::new(LogicalQty(2), LogicalQty(8));
		let event_id: event::ID = (addr, LogicalQty(0)).into();
		index.insert(event_id, StorageQty(0)).unwrap();
		index.commit().unwrap();

		assert_eq!(index.get(event_id), Some(StorageQty(0)));
	}

	proptest! {
		#[test]
		fn txn_either_succeeds_or_fails(
			vs in proptest::collection::vec(
				(arb_event_id(), arb_storage_pos()),
				0..1000),
			txn_events_per_addr in 0usize..10usize,
			actual_events_per_addr in 0usize..200usize

		) {
			let mut index =
				Index::new(txn_events_per_addr, actual_events_per_addr);

			let original_index = index.clone();

			let res: Result<(), InsertErr> = vs
				.iter()
				.map(|&(event_id, disk_offset)| {
					index.insert(event_id, disk_offset)
				})
				.collect();

			if let Err(_) = res {
				index.rollback();
				assert_eq!(original_index, index);
			} else if let Err(_) = index.commit() {
				index.rollback();
				assert_eq!(original_index, index);
			} else {
				let actual: alloc::vec::Vec<(event::ID, StorageQty)> =
					vs.iter()
						.filter(|&(event_id, _)| index.get(*event_id).is_some())
						.copied()
						.collect();

				assert_eq!(vs, actual);
			}
		}

	}
}
