//! Module exists purely to prevent circular dependency.

use core::fmt;
use derive_more::Add;

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
	pub fn new<R: rand::Rng>(rng: &mut R) -> Self {
		Self([rng.gen(), rng.gen()])
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

/// Position of a word, in either memory or disk
#[derive(
	Add,
	Clone,
	Copy,
	Debug,
	Default,
	PartialEq,
	PartialOrd,
	bytemuck::Pod,
	bytemuck::Zeroable,
)]
#[repr(transparent)]
pub struct StorageQty(pub usize);

/// Logical Position of the event on the log, ie the 'nth' event
#[derive(
	Add,
	Clone,
	Copy,
	Debug,
	Eq,
	PartialOrd,
	Ord,
	PartialEq,
	bytemuck::Pod,
	bytemuck::Zeroable,
)]
#[repr(transparent)]
pub struct LogicalQty(pub usize);

impl LogicalQty {
	pub fn is_initial(&self) -> bool {
		self.0 == 0
	}
}
