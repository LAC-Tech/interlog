//! Module exists purely to prevent circular dependency.

use core::fmt;
use derive_more::{Add, Into};
use rand::prelude::*;

/// This was originally u128, but I changed it to keep the alignment to 0x8
#[derive(
	bytemuck::Pod,
	bytemuck::Zeroable,
	Clone,
	Copy,
	Default,
	Eq,
	Hash,
	PartialEq,
	PartialOrd,
	Ord,
)]
#[repr(transparent)]
pub struct Addr([u64; 2]);

impl Addr {
	pub fn new<R: Rng>(rng: &mut R) -> Self {
		Self(rng.gen())
	}
}

impl fmt::Display for Addr {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:x}{:x}", self.0[0], self.0[1])
	}
}

impl fmt::Debug for Addr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "ReplicaID({:x}{:x})", self.0[0], self.0[1])
	}
}

#[derive(Add, Clone, Copy, Debug, Default, Into, PartialEq)]
pub struct DiskOffset(usize);

impl DiskOffset {
	pub fn is_initial(&self) -> bool {
		self.0 == 0
	}
}

#[derive(Add, Clone, Copy, Debug, PartialEq)]
pub struct LogicalPos(usize);

impl LogicalPos {
	pub fn consecutive(self, n: Self) -> bool {
		self.0 + 1 == n.0
	}
}
