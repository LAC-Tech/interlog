//! "Domain integers" - an attempt to bring something like F#s unit of measure.
use core::fmt;
use derive_more::*;

/// Represents a byte address, divisible by 8, where an Event starts.
#[repr(transparent)]
#[derive(
	Add, AddAssign, Clone, Copy, From, Into, PartialEq, PartialOrd, Sub,
)]
pub struct Byte(pub usize);

impl Byte {
	pub fn align(self) -> Byte {
		Self((self.0 + 7) & !7)
	}
}

impl fmt::Display for Byte {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{} <byte>", self.0)
	}
}

impl fmt::Debug for Byte {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:?}", self.0)
	}
}

/// The sequence number or position on the log of an event.
#[repr(transparent)]
#[derive(
	Add, AddAssign, Clone, Copy, From, Into, bytemuck::Pod, bytemuck::Zeroable,
)]
pub struct Logical(pub usize);

impl fmt::Display for Logical {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{} <logical>", self.0)
	}
}

impl fmt::Debug for Logical {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:?}", self.0)
	}
}
