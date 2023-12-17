#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes 64 bit");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use std::io::Read;

use bytemuck;
use rand::prelude::*;
use rustix;

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

#[repr(align(2))]
pub struct LogOffset([u16; 3]);

struct EventID {
	origin: ReplicaID,
	offset: LogOffset,
}

#[repr(align(8))]
struct EventHeader {
	length: u32, // in bytes, approx 34Gb
	_unused: u16,
	origin: ReplicaID,
}

// limit rustix to this module?
mod disk {
	use crate::rustix::{fd, fs, io};
	use fs::OFlags;

	pub struct Log(fd::OwnedFd);

	impl Log {
		pub fn new<P: rustix::path::Arg>(path: P) -> Result<Self, io::Errno> {
			let fd = fs::open(
				path,
				OFlags::DIRECT | OFlags::CREATE | OFlags::APPEND | OFlags::RDWR,
				fs::Mode::RUSR | fs::Mode::WUSR,
			)?;

			Ok(Self(fd))
		}

		pub fn write(&self, bytes: &[u8]) -> io::Result<usize> {
			// always sets file offset to EOF.
			io::write(&self.0, bytes)
		}

		pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
			// pread ignores the file offset
			io::pread(&self.0, buf, 0)
		}
	}
}

pub struct LocalReplica {
	pub id: ReplicaID,
	pub path: std::path::PathBuf,
	log: disk::Log,
	id_index: Vec<LogOffset>,
}

impl LocalReplica {
	pub fn new<R: Rng>(rng: &mut R) -> Result<Self, rustix::io::Errno> {
		let id = ReplicaID::new(rng);
		let path_str = format!("/tmp/interlog/{}", id);
		let path = std::path::PathBuf::from(path_str);
		let log = disk::Log::new(&path)?;
		let id_index = vec![];
		Ok(Self { id, path, log, id_index })
	}

	pub fn write(&self, data: &[u8]) -> Result<usize, rustix::io::Errno> {
		self.log.write(data)
	}

	pub fn read(&self, buf: &mut [u8]) -> Result<usize, rustix::io::Errno> {
		self.log.read(buf)
	}
}

fn main() {
	let mut rng = rand::thread_rng();
	let rid = ReplicaID::new(&mut rng);
	let eh = EventHeader { origin: rid, length: 0, _unused: 0 };
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn read_and_write_to_log() {
		let mut rng = rand::thread_rng();
		let replica = LocalReplica::new(&mut rng).expect("failed to open file");
		replica.write("Hello,\n").expect("failed to write to replica");
		replica.write("world!\n").expect("failed to write to replica");

		let mut buf = [0; 13];
		replica.read(&mut buf).expect("failed to read to file");
		assert_eq!(&buf, b"Hello,\nworld!");
		let path = replica.path.clone();
		std::fs::remove_file(path).expect("failed to remove file");
	}
}
