//! Module exists purely to prevent circular dependency.

use core::fmt;

/// This was originally u128, but I changed it to keep the alignment to 0x8
#[derive(
	bytemuck::Pod,
	bytemuck::Zeroable,
	Clone,
	Copy,
	Default,
	Eq,
	derive_more::From,
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
		write!(f, "{:x}{:x}", self.0[0], self.0[1])
	}
}

/// Logical Position of the event on the log, ie the 'nth' event
#[derive(
	derive_more::Add,
	derive_more::AddAssign,
	Clone,
	Copy,
	Default,
	Eq,
	PartialOrd,
	Ord,
	PartialEq,
	bytemuck::Pod,
	bytemuck::Zeroable,
)]
#[repr(transparent)]
pub struct LogicalQty(pub usize);

impl core::fmt::Debug for LogicalQty {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}<logical>", self.0)
	}
}
