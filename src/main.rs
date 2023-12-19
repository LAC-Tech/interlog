#![feature(vec_push_within_capacity)]

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes 64 bit");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use fs::OFlags;
use rand::prelude::*;
use rustix::{fd, fs, io};

#[derive(Clone, Copy)]
#[repr(align(2))]
pub struct ReplicaID([u16; 5]);

impl ReplicaID {
	fn new<R: Rng>(rng: &mut R) -> Self {
		Self(rng.gen())
	}
}

impl core::fmt::Display for ReplicaID {
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		for n in self.0 {
			write!(f, "{:x}", n)?;
		}

		Ok(())
	}
}

// Virtual Sector Length
const VSLEN: usize = 4096;

pub struct LocalReplica {
	pub id: ReplicaID,
	pub path: std::path::PathBuf,
	log_fd: fd::OwnedFd,
	read_cache: Vec<u8>,
	write_cache: Vec<u8>,
}

impl LocalReplica {
	pub fn new<R: Rng>(rng: &mut R) -> Result<Self, rustix::io::Errno> {
		let id = ReplicaID::new(rng);
		let path_str = format!("/tmp/interlog/{}", id);
		let path = std::path::PathBuf::from(path_str);
		let log_fd = fs::open(
			&path,
			OFlags::DIRECT | OFlags::CREATE | OFlags::APPEND | OFlags::RDWR,
			fs::Mode::RUSR | fs::Mode::WUSR,
		)?;
		let read_cache = Vec::with_capacity(VSLEN);
		let write_cache = Vec::with_capacity(VSLEN);
		Ok(Self { id, path, log_fd, read_cache, write_cache })
	}

	pub fn write(&self, data: &[u8]) -> Result<usize, rustix::io::Errno> {
		// always sets file offset to EOF.
		io::write(&self.log_fd, data)
	}

	pub fn read(&mut self) -> Result<&[u8], rustix::io::Errno> {
		self.read_cache.clear();
		// pread ignores the file offset
		io::pread(&self.log_fd, &mut self.read_cache, 0)?;
		Ok(&self.read_cache)
	}
}

fn main() {
	let mut rng = rand::thread_rng();
	let rid = ReplicaID::new(&mut rng);
	let mut ns: Vec<u8> = vec![];
	ns.push_within_capacity(9);
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn read_and_write_to_log() {
		let mut rng = rand::thread_rng();
		let replica = LocalReplica::new(&mut rng).expect("failed to open file");
		replica.write(b"Hello,\n").expect("failed to write to replica");
		replica.write(b"world!\n").expect("failed to write to replica");

		let buf = [0; 13] = replica.read().expect("failed to read to file");
		assert_eq!(&buf, b"Hello,\nworld!");
		let path = replica.path.clone();
		std::fs::remove_file(path).expect("failed to remove file");
	}
}
