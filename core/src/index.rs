use crate::event;
use crate::fixed_capacity;
use crate::fixed_capacity::Vec;
use crate::pervasives::*;

use hashbrown::HashMap;

#[derive(Debug)]
pub struct Capacities {
	// pub addrs: usize,
}

#[derive(Debug)]
struct Elem {
	txn: Vec<DiskOffset>,
	actual: Vec<DiskOffset>,
}

#[derive(Debug)]
pub struct Index {
	// TODO: fixed size hashmap
	map: HashMap<Addr, Elem>,
	txn_events_per_addr: usize,
	actual_events_per_addr: usize,
}
/// In-memory mapping of event IDs to disk offsets
/// This keeps the following invariants:
/// - events must be stored consecutively per address
/// This effectively stores the causal histories over every addr
impl Index {
	pub fn new(
		txn_events_per_addr: usize,
		actual_events_per_addr: usize,
	) -> Self {
		// TODO: HOW MANY ADDRS WILL I HAVE?
		Self {
			map: HashMap::new(),
			txn_events_per_addr,
			actual_events_per_addr,
		}
	}

	pub fn enqueue<D: Into<DiskOffset>>(
		&mut self,
		event_id: event::ID,
		disk_offset: D,
	) -> Result<(), EnqueueErr> {
		let disk_offset: DiskOffset = disk_offset.into();
		match self.map.get_mut(&event_id.origin) {
			Some(existing) => {
				if existing.txn.len() != event_id.log_pos.0 {
					return Err(EnqueueErr::NonConsecutivePos);
				}

				if let Some(last_offset) = existing.txn.last() {
					if *last_offset >= disk_offset {
						panic!("non-monotonic disk offset")
					}
				}

				existing.txn.push(disk_offset).map_err(|err| match err {
					fixed_capacity::Overflow => EnqueueErr::Overflow,
				})
			}
			None => {
				if event_id.log_pos.0 != 0 {
					return Err(EnqueueErr::IndexWouldNotStartAtZero);
				}

				let new_elem = Elem {
					txn: Vec::from_elem(disk_offset, self.txn_events_per_addr),
					actual: Vec::new(self.actual_events_per_addr),
				};

				self.map.insert(event_id.origin, new_elem);
				Ok(())
			}
		}
	}

	pub fn commit(&mut self) -> Result<(), CommitErr> {
		let enough_space = self
			.map
			.values()
			.all(|Elem { txn, actual }| actual.can_be_extended_by(&txn));

		if !enough_space {
			return Err(CommitErr::NotEnoughSpace);
		}

		for Elem { txn, actual } in self.map.values_mut() {
			actual
				.extend_from_slice(&txn)
				.expect("FATAL ERR: partially committed transaction");
			txn.clear();
		}

		Ok(())
	}

	pub fn rollback(&mut self) {
		for Elem { txn, .. } in self.map.values_mut() {
			txn.clear()
		}
	}

	pub fn get(&self, event_id: event::ID) -> Option<DiskOffset> {
		let result = self
			.map
			.get(&event_id.origin)
			.and_then(|elem| elem.actual.get(event_id.log_pos.0));

		result.cloned()
	}
}

#[derive(Debug, PartialEq)]
pub enum EnqueueErr {
	NonConsecutivePos,
	IndexWouldNotStartAtZero,
	Overflow,
}

#[derive(Debug, PartialEq)]
pub enum CommitErr {
	NotEnoughSpace,
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
		let mut index = Index::new(2, 8);

		let actual = index.enqueue(event::ID::new(addr, 0), 0);
		assert_eq!(actual, Ok(()));

		let actual = index.enqueue(event::ID::new(addr, 34), 0);
		assert_eq!(actual, Err(EnqueueErr::NonConsecutivePos));
	}

	#[test]
	fn index_would_not_start_at_zero() {
		let mut rng = thread_rng();
		let addr = Addr::new(rng.gen());
		let mut index = Index::new(2, 8);

		let actual = index.enqueue(event::ID::new(addr, 42), 0);
		assert_eq!(actual, Err(EnqueueErr::IndexWouldNotStartAtZero));
	}

	#[test]
	fn overlow() {
		let mut rng = thread_rng();
		let addr = Addr::new(rng.gen());
		let mut index = Index::new(1, 8);

		let actual = index.enqueue(event::ID::new(addr, 0), 0);
		assert_eq!(actual, Ok(()));

		let actual = index.enqueue(event::ID::new(addr, 1), 1);
		assert_eq!(actual, Err(EnqueueErr::Overflow));
	}

	#[test]
	fn not_enough_space() {
		let mut rng = thread_rng();
		let addr = Addr::new(rng.gen());
		let mut index = Index::new(2, 1);

		let actual = index.enqueue(event::ID::new(addr, 0), 0);
		assert_eq!(actual, Ok(()));

		let actual = index.enqueue(event::ID::new(addr, 1), 1);
		assert_eq!(actual, Ok(()));

		let actual = index.commit();
		assert_eq!(actual, Err(CommitErr::NotEnoughSpace))
	}

	proptest! {
		#[test]
		fn txn_either_succeeds_or_fails(
			vs in proptest::collection::vec(
				(arb_addr(), arb_log_pos(), arb_disk_offset()),
				0..1000),
			txn_events_per_addr in any::<usize>(),
			actual_events_per_addr in any::<usize>()

		){
			let mut index = Index::new(txn_events_per_addr, actual_events_per_addr);

			for (addr, log_pos, disk_offset) in vs {
				index.enqueue(event::ID::new(addr, log_pos), disk_offset);
			}

		}

	}
}
