//! Module exists purely to prevent circular dependency.

use core::fmt;
use derive_more::{Add, From, Into};
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
	pub fn new(rand_data: [u64; 2]) -> Self {
		Self(rand_data)
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

/// Byte position of an event, on disk
#[derive(
	Add,
	Clone,
	Copy,
	Debug,
	Default,
	From,
	PartialEq,
	PartialOrd,
	bytemuck::Pod,
	bytemuck::Zeroable,
)]
#[repr(transparent)]
pub struct DiskOffset(pub usize);

impl DiskOffset {
	pub fn is_initial(&self) -> bool {
		self.0 == 0
	}
}

/// Logical Position of the event on the log, ie the 'nth' event
#[derive(
	Add,
	Clone,
	Copy,
	Debug,
	Eq,
	From,
	PartialOrd,
	Ord,
	PartialEq,
	bytemuck::Pod,
	bytemuck::Zeroable,
)]
#[repr(transparent)]
pub struct LogPos(pub usize);

impl LogPos {
	pub fn consecutive(self, n: Self) -> bool {
		self.0 + 1 == n.0
	}
}
