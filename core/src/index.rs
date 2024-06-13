use crate::event;
use crate::fixed_capacity::Vec;
use crate::pervasives::*;

use hashbrown::HashMap;

// TODO: 'permanent' index, and 'staging' index I can use to calculate if I will commit or not
#[derive(Debug)]
struct Index(HashMap<Addr, Elem>);

impl Index {
	fn new() -> Self {
		Self(HashMap::new())
	}

	fn enqueue(&mut self, event_id: event::ID) -> Result<(), ParseErr> {
		panic!("TODO: implement me")
	}

	fn commit(&mut self) {
		panic!("TODO: implement me")
	}
}

#[derive(Debug)]
struct Elem {
	// This way I can pre-allocate memory
	txn_buf: Vec<DiskOffset>,
	actual: Vec<DiskOffset>,
}

fn is_consecutive(ns: &[LogicalPos]) -> bool {
	ns.iter().try_reduce(|prev, n| prev.consecutive(*n).then(|| n)).is_some()
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
}
