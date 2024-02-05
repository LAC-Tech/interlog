use rand::prelude::*;

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
#[repr(transparent)]
pub struct ReplicaID(u128);

impl ReplicaID {
    pub fn new<R: Rng>(rng: &mut R) -> Self {
        Self(rng.gen())
    }
}

impl core::fmt::Display for ReplicaID {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{:x}", self.0)
    }
}
