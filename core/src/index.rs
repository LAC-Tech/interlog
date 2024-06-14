use crate::event;
use crate::fixed_capacity;
use crate::fixed_capacity::Vec;
use crate::pervasives::*;

use hashbrown::HashMap;

#[derive(Debug)]
pub struct Capacities {
	pub addrs: usize,
	pub txn_events_per_addr: usize,
	pub actual_events_per_addr: usize,
}

#[derive(Debug)]
struct Elem {
	txn: Vec<DiskOffset>,
	actual: Vec<DiskOffset>,
}

#[derive(Debug)]
pub struct Index {
	map: HashMap<Addr, Elem>,
	capacities: Capacities,
}
/// In-memory mapping of event IDs to disk offsets
/// This keeps the following invariants:
/// - events must be stored consecutively per address
/// This effectively stores the causal histories over every addr
impl Index {
	pub fn new(capacities: Capacities) -> Self {
		// TODO: HOW MANY ADDRS WILL I HAVE?
		Self { map: HashMap::new(), capacities }
	}

	pub fn enqueue(
		&mut self,
		event_id: event::ID,
		disk_offset: DiskOffset,
	) -> Result<(), EnqueueErr> {
		match self.map.get_mut(&event_id.origin) {
			Some(existing) => {
				if existing.txn.len() == event_id.log_pos.into() {
					existing.txn.push(disk_offset).map_err(EnqueueErr::Overflow)
				} else {
					Err(EnqueueErr::NonConsecutive)
				}
			}
			None => {
				if !disk_offset.is_initial() {
					return Err(EnqueueErr::IndexWouldNotStartAtZero);
				}

				let new_elem = Elem {
					txn: Vec::from_elem(
						disk_offset,
						self.capacities.txn_events_per_addr,
					),

					actual: Vec::new(self.capacities.actual_events_per_addr),
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
		let result = self.map.get(&event_id.origin).and_then(|elem| {
			let i: usize = event_id.log_pos.into();
			elem.actual.get(i)
		});

		result.cloned()
	}
}

pub enum EnqueueErr {
	NonConsecutive,
	IndexWouldNotStartAtZero,
	Overflow(fixed_capacity::Overflow),
}

pub enum CommitErr {
	NotEnoughSpace,
}

#[cfg(test)]
mod tests {
	use super::*;
	//use crate::test_utils::*;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;

	#[test]
	fn f() {
		let mut rng = thread_rng();
		let addrs: [Addr; 2] = core::array::from_fn(|_| Addr::new(&mut rng));
	}

	//let index = Index::new();
}
