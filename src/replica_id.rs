use core::fmt;
use rand::prelude::*;

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
#[repr(transparent)]
pub struct ReplicaID(u128);

impl ReplicaID {
	pub fn new<R: Rng>(rng: &mut R) -> Self {
		Self(rng.gen())
	}
}

impl fmt::Display for ReplicaID {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "ReplicaID({:x})", self.0)
	}
}

impl fmt::Debug for ReplicaID {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:x}", self.0)
	}
}
