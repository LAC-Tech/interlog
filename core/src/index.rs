use core::ops::Range;

use crate::event;
use crate::fixed_capacity::Vec;
use crate::mem;
use crate::pervasives::*;
use crate::storage;

/// Version Vector
mod version_vector {
	use crate::event;
	use crate::pervasives::*;
	use hashbrown::hash_map::Entry;
	use hashbrown::HashMap;

	// TODO: fixed capacity hash map, if such a thing is even possible
	#[derive(Clone, Debug, PartialEq)]
	pub struct VersionVector(HashMap<Addr, usize>);

	impl VersionVector {
		pub fn new() -> Self {
			Self(HashMap::new())
		}

		fn get_raw(&self, addr: Addr) -> usize {
			self.0.get(&addr).copied().unwrap_or(0)
		}

		pub fn get(&self, addr: Addr) -> LogicalQty {
			// If an address it not the VV, by definition there are 0 updates
			LogicalQty(self.get_raw(addr))
		}

		pub fn insert(&mut self, eid: event::ID) {
			let new_count = eid.pos.0 + 1;
			let addr = eid.origin;
			self.0
				.entry(addr)
				.and_modify(|count| {
					let expected = *count + 1;
					assert_eq!(expected, new_count);
					*count = new_count;
				})
				.or_insert_with(|| {
					if new_count != 1 {
						panic!("new count was not one");
					}

					new_count
				});
		}

		pub fn transfer_count(&mut self, src: &VersionVector, addr: Addr) {
			self.0
				.entry(addr)
				.or_insert_with(|| *src.0.get(&addr).unwrap_or(&0));
		}

		pub fn merge_in(&mut self, txn: &VersionVector) {
			for (&addr, &new_counter) in txn.0.iter() {
				self.0.entry(addr).and_modify(|current_counter| {
					if new_counter > *current_counter {
						*current_counter = new_counter;
					} else {
						panic!("Txn VV does not dominate actual VV")
					}
				});
			}
		}

		pub fn clear(&mut self) {
			self.0.clear();
		}

		// Does the LHS dominate the RHS?
		pub fn dominates(&self, other: &VersionVector) -> bool {
			// looping over both to avoid storing union of both adddrs
			for (&addr, &left) in self.0.iter() {
				let right = other.get_raw(addr);
				if right > left {
					return false;
				}
			}

			for (&addr, &right) in other.0.iter() {
				let left = self.get_raw(addr);
				if right > left {
					return false;
				}
			}

			true
		}
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct Index {
	txn_buf: Vec<storage::Qty>,
	logical_to_storage: Vec<storage::Qty>,
	actual_vv: version_vector::VersionVector,
	txn_vv: version_vector::VersionVector,
}
impl Index {
	pub fn new(max_txn_size: LogicalQty, max_events: LogicalQty) -> Self {
		// TODO: HOW MANY ADDRS WILL I HAVE?
		Self {
			logical_to_storage: Vec::new(max_events.0),
			txn_buf: Vec::new(max_txn_size.0),
			actual_vv: version_vector::VersionVector::new(),
			txn_vv: version_vector::VersionVector::new(),
		}
	}

	pub fn enqueue(
		&mut self,
		e: &event::Event,
		stored_offset: storage::Qty,
	) -> Result<(), EnqueueErr> {
		self.txn_vv.transfer_count(&self.actual_vv, e.id.origin);
		self.txn_vv.insert(e.id);

		let offset =
			stored_offset + self.txn_buf.iter().copied().sum() + e.size();

		if let Err(mem::Overrun) = self.txn_buf.push(offset) {
			return Err(EnqueueErr {
				kind: EnqueueErrKind::Overrun,
				event_id: e.id,
			});
		}

		Ok(())
	}

	pub fn commit(&mut self) -> Result<(), CommitErr> {
		self.actual_vv.merge_in(&self.txn_vv);
		if let Err(mem::Overrun) =
			self.logical_to_storage.extend_from_slice(&self.txn_buf)
		{
			return Err(CommitErr::Overrun);
		}
		Ok(())
	}

	pub fn rollback(&mut self) {
		self.txn_buf.clear();
		self.txn_vv.clear();
	}

	pub fn read(&self, logical: Range<LogicalQty>) -> &[storage::Qty] {
		&self.logical_to_storage[logical.start.0..logical.end.0]
	}
}

#[derive(Debug)]
pub struct EnqueueErr {
	kind: EnqueueErrKind,
	event_id: event::ID,
}

#[derive(Debug, PartialEq)]
pub enum EnqueueErrKind {
	Overrun,
}

#[derive(Debug, PartialEq)]
pub enum CommitErr {
	Overrun,
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::event::Event;
	use crate::test_utils::*;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;
	use rand::prelude::*;

	use version_vector::NonConsecutiveErr;
	use EnqueueErr::*;

	// Sanity check unit tests - trigger every error

	#[test]
	fn non_consecutive() {
		let mut rng = thread_rng();
		let addr = Addr::new(&mut rng);
		let mut index = Index::new(LogicalQty(2), LogicalQty(8));

		let offset = storage::Qty(0);

		let actual =
			index.enqueue(&Event::new(addr, LogicalQty(0), b"non"), offset);
		assert_eq!(actual, Ok(()));

		let actual = index.enqueue(
			&Event::new(addr, LogicalQty(34), b"consecutive"),
			storage::Qty(0),
		);
		assert_eq!(actual, Err(VersionVector(NonConsecutiveErr)))
	}

	#[test]
	fn index_would_not_start_at_zero() {
		let mut rng = thread_rng();
		let addr = Addr::new(&mut rng);
		let mut index = Index::new(LogicalQty(2), LogicalQty(8));

		let actual = index.enqueue(
			&Event::new(addr, LogicalQty(42), b"not at zero"),
			storage::Qty(0),
		);
		assert_eq!(actual, Err(VersionVector(NonConsecutiveErr)));
	}

	#[test]
	fn enqueue_overrun() {
		let mut rng = thread_rng();
		let addr = Addr::new(&mut rng);
		let mut index = Index::new(LogicalQty(1), LogicalQty(8));

		let offset = storage::Qty(0);
		let actual =
			index.enqueue(&Event::new(addr, LogicalQty(0), b"over"), offset);
		assert_eq!(actual, Ok(()));

		let actual =
			index.enqueue(&Event::new(addr, LogicalQty(1), b"run"), offset);
		assert_eq!(actual, Err(EnqueueErr::Overrun));
	}

	#[test]
	fn commit_overrun() {
		let mut rng = thread_rng();
		let addr = Addr::new(&mut rng);
		let mut index = Index::new(LogicalQty(2), LogicalQty(1));
		let offset = storage::Qty(0);
		let actual =
			index.enqueue(&Event::new(addr, LogicalQty(0), b"commit"), offset);
		assert_eq!(actual, Ok(()));

		let actual =
			index.enqueue(&Event::new(addr, LogicalQty(1), b"overrun"), offset);
		assert_eq!(actual, Ok(()));

		let actual = index.commit();
		assert_eq!(actual, Err(CommitErr::Overrun))
	}

	#[test]
	fn enqueue_and_get() {
		let mut rng = thread_rng();
		let addr = Addr::new(&mut rng);
		let mut index = Index::new(LogicalQty(2), LogicalQty(8));
		let offset = storage::Qty(0);
		let event = Event::new(addr, LogicalQty(0), b"enqueue and get");
		index.enqueue(&event, offset).unwrap();
		index.commit().unwrap();

		assert_eq!(
			index.read(LogicalQty(0)..LogicalQty(1)),
			&[offset + event.size()]
		);
	}

	proptest! {
		#[test]
		fn proptest_enqueue_and_get(
			offset in 0usize..10_000_000usize,
			payload in arb_payload(4096)
		) {
			let mut rng = thread_rng();
			let addr = Addr::new(&mut rng);
			let mut index = Index::new(LogicalQty(2), LogicalQty(8));
			let offset = storage::Qty(offset);
			let event = Event::new(addr, LogicalQty(0), &payload);
			index.enqueue(&event, offset).unwrap();
			index.commit().unwrap();

			assert_eq!(
				index.read(LogicalQty(0)..LogicalQty(1)),
				&[offset + event.size()]
			);
		}

		#[test]
		fn txn_either_succeeds_or_fails(
			id_payload_pairs in proptest::collection::vec(
				(arb_event_id(), arb_payload(100)),
				0..1000),
			max_txn_size in 0usize..10usize,
			max_events in 0usize..200usize,
			offset in 0usize..10_000_000usize
		) {
			let mut index =
				Index::new(
					LogicalQty(max_txn_size),
					LogicalQty(max_events));

			let control_index = index.clone();

			let res: Result<(), EnqueueErr> = id_payload_pairs
				.iter()
				.map(|(id, payload)| {
					let e = Event {id: *id, payload};
					index.enqueue(&e, storage::Qty(offset))
				})
				.collect();

			if let Err(_) = res {
				index.rollback();
				assert_eq!(control_index, index);
			} else if let Err(_) = index.commit() {
				index.rollback();
				assert_eq!(control_index, index, "after commit err, rolling back still has the two indexes in an inconsistent state");
			} else {

				let actual = index.read(
					LogicalQty(0)..LogicalQty(id_payload_pairs.len())
				);

				let expected: alloc::vec::Vec<storage::Qty> = id_payload_pairs
					.iter().map(|(id, payload)| {
						let e = Event {id: *id, payload};
						e.size()
					}).collect();

				assert_eq!(actual, expected)
			}
		}
	}
}
