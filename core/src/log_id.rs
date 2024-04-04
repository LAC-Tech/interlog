//! Module exists purely to prevent circular dependency.
use core::fmt;
use rand::prelude::*;

/// This was originally u128, but I changed it to keep the alignment to 0x8
#[derive(
	bytemuck::Pod,
	bytemuck::Zeroable,
	Clone,
	Copy,
	Eq,
	PartialEq,
	PartialOrd,
	Ord,
)]
#[repr(transparent)]
pub struct LogID([u64; 2]);

impl LogID {
	pub fn new<R: Rng>(rng: &mut R) -> Self {
		Self(rng.gen())
	}
}

impl fmt::Display for LogID {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:x}{:x}", self.0[0], self.0[1])
	}
}

impl fmt::Debug for LogID {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "ReplicaID({:x}{:x})", self.0[0], self.0[1])
	}
}
