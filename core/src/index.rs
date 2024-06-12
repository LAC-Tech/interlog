use rand::prelude::*;

use crate::event;
use crate::fixed_capacity::Vec;
use crate::pervasives::*;

// TODO: 'permanent' index, and 'staging' index I can use to calculate if I will commit or not
#[derive(Debug)]
struct Index<const MAX_ADDRS: usize, const MAX_EVENTS_PER_ADDR: usize> {
	// This way I can pre-allocate memory
	txn_buf: Vec<(Addr, Vec<DiskOffset, MAX_EVENTS_PER_ADDR>), MAX_ADDRS>,
	actual: Vec<(Addr, Vec<DiskOffset, MAX_EVENTS_PER_ADDR>), MAX_ADDRS>,
}

impl<const MAX_ADDRS: usize, const MAX_EVENTS_PER_ADDR: usize>
	Index<MAX_ADDRS, MAX_EVENTS_PER_ADDR>
{
	fn new() -> Self {
		Self { txn_buf: Vec::new(), actual: Vec::new() }
	}

	fn extend(&mut self, eids: &[event::ID]) -> Result<(), ParseErr> {
		panic!("TODO: implement me")
	}
}

fn is_consecutive(ns: &[DiskOffset]) -> bool {
	ns.iter().try_reduce(|prev, n| (*n == *prev + 1).then(|| n)).is_some()
}

enum ParseErr {
	NonConsecutive(Addr),
	IndexWouldNotStartAtZero(Addr),
}

#[cfg(test)]
mod tests {
	use super::*;
	//use crate::test_utils::*;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;

	#[test]
	fn empty_slice_is_consecutive() {
		assert_eq!(is_consecutive(&[]), true);
	}

	proptest! {
		#[test]
		fn singleton_slice_is_consecutive(n in any::<DiskOffset>()) {
			assert_eq!(is_consecutive(&[n]), true);
		}
	}

	#[test]
	fn monotonic_does_not_imply_consecutive() {
		assert_eq!(is_consecutive(&[1, 2, 99, 100]), false);
	}

	#[test]
	fn positive_consecutive() {
		assert_eq!(is_consecutive(&[50, 49, 48, 47]), false);
	}

	#[test]
	fn f() {
		let mut rng = thread_rng();
		let addrs: [Addr; 2] = core::array::from_fn(|_| Addr::new(&mut rng));
		let eids = [event::ID { origin: addrs[0], log_pos: 0 }];
	}

	//let index = Index::new();
}
