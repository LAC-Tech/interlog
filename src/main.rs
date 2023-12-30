#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use fs::OFlags;
use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};
use std::io::Read;

type O = OFlags;

struct ReplicaID(u128);

impl ReplicaID {
	fn new<R: Rng>(rng: &mut R) -> Self {
		Self(rng.gen())
	}
}

impl core::fmt::Display for ReplicaID {
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		write!(f, "{:x}", self.0)
	}
}

trait Replica {
	// Events local to the replica, that don't yet have an ID
	fn write_local<const N: usize>(
		&mut self,
		events: [&[u8]; N],
	) -> [ReplicaID; N];

	// Events that have already been recorded on other replicas
	// Designed to be used by the sync protocol
	fn write_remote(&mut self);
}
