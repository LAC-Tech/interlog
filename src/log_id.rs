//! Module exists purely to prevent circular dependency.
use core::fmt;
use rand::prelude::*;

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
#[repr(transparent)]
pub struct LogID(u128);

impl LogID {
	pub fn new<R: Rng>(rng: &mut R) -> Self {
		Self(rng.gen())
	}
}

impl fmt::Display for LogID {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ReplicaID({:x})", self.0)
	}
}

impl fmt::Debug for LogID {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:x}", self.0)
	}
}
