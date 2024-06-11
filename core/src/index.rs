use hashbrown::HashMap;
use itertools::Itertools;

use crate::event;
use crate::fixvec::FixVec;
use crate::log_id::LogID;

type DiskOffset = usize;

// TODO: 'permanent' index, and 'staging' index I can use to calculate if I will commit or not
#[derive(Debug)]
struct Index {
	// This way I can pre-allocate memory
	txn_buf: HashMap<LogID, FixVec<DiskOffset>>,
	actual: HashMap<LogID, FixVec<DiskOffset>>,
}

fn is_consecutive(ns: &[DiskOffset]) -> bool {
	if ns.len() < 2 {
		return true;
	}

	ns[1..]
		.iter()
		.try_fold(ns[0], |prev, &n| (n == prev + 1).then(|| n))
		.is_some()
}

enum ParseErr {
	NonConsecutive(LogID),
	IndexWouldNotStartAtZero(LogID),
}

/*
fn parse_event_ids(eids: &[event::ID]) -> ParseErr {
	let by_origin = eids.iter().chunk_by(|e| e.origin);
}
*/

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
