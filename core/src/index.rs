use rand::prelude::*;

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
pub struct Index {
	map: HashMap<Addr, Elem>,
	capacities: Capacities,
}
/// In-memory mapping of event IDs to disk offsets
/// This keeps the following invariants:
/// - events must be stored consecutively per address
/// This effectively stores the Causal Histories over every addr
impl Index {
	pub fn new(capacities: Capacities) -> Self {
		Self { map: HashMap::new(), capacities }
	}

	pub fn enqueue(
		&mut self,
		event_id: event::ID,
		disk_offset: DiskOffset,
	) -> Result<(), QueueErr> {
		match self.map.get_mut(&event_id.origin) {
			Some(existing) => {
				if existing.txn.len() == event_id.log_pos.into() {
					existing.txn.push(disk_offset).map_err(QueueErr::Overflow)
				} else {
					Err(QueueErr::NonConsecutive)
				}
			}
			None => {
				if disk_offset.is_initial() {
					let new_elem = Elem {
						txn: Vec::from_elem(
							disk_offset,
							self.capacities.txn_events_per_addr,
						),

						actual: Vec::new(
							self.capacities.actual_events_per_addr,
						),
					};

					self.map.insert(event_id.origin, new_elem);
					Ok(())
				} else {
					Err(QueueErr::IndexWouldNotStartAtZero)
				}
			}
		}
	}

	pub fn commit(&mut self) {
		panic!("TODO: implement me")
	}
}

#[derive(Debug)]
struct Elem {
	txn: Vec<DiskOffset>,
	actual: Vec<DiskOffset>,
}

enum QueueErr {
	NonConsecutive,
	IndexWouldNotStartAtZero,
	Overflow(fixed_capacity::Overflow),
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
