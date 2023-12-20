#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes 64 bit");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use fs::OFlags;
use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};
use std::io::Read;

type O = OFlags;

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

#[derive(Clone, Copy)]
#[repr(transparent)]
struct EventLen(u8);

impl From<usize> for EventLen {
	fn from(val: usize) -> Self {
		let n: u8 = val.try_into().expect("Event len was bigger than 2^8");
		EventLen(n)
	}
}

impl From<EventLen> for usize {
	fn from(val: EventLen) -> Self {
		val.0.into()
	}
}

#[repr(align(8))]
struct EventID {
	origin: ReplicaID,
	log_offset: u32,
	len: EventLen,
}

// Virtual Sector Size
const VS_SIZE: usize = 256;

// Virtual Sector
pub struct VSect {
	bytes: [u8; VS_SIZE],
	pos: usize,
}

impl VSect {
	pub fn new() -> Self {
		Self { bytes: [0u8; VS_SIZE], pos: 0 }
	}

	pub fn write(&mut self, data: &[u8]) {
		let new_pos = self.pos + data.len();
		self.bytes[self.pos..new_pos].copy_from_slice(data);
		self.pos = new_pos;
	}

	pub fn as_bytes(&self) -> &[u8] {
		&self.bytes[0..self.pos]
	}

	fn fetch(
		&mut self,
		fd: fd::BorrowedFd,
		len: EventLen,
	) -> rustix::io::Result<usize> {
		// pread ignores the file offset
		let bytes_read = io::pread(fd, &mut self.bytes[0..len.into()], 0)?;
		self.pos = bytes_read;
		Ok(bytes_read)
	}

	fn flush(&mut self, fd: fd::BorrowedFd) -> rustix::io::Result<u32> {
		// always sets file offset to EOF.
		let bytes_written = io::write(fd, &self.bytes)?;
		// Resetting
		self.bytes[0..self.pos].fill(0);
		self.pos = 0;
		Ok(bytes_written.try_into().expect("wrote more than 2^32 bytes"))
	}
}

impl Default for VSect {
	fn default() -> Self {
		Self::new()
	}
}

pub struct LocalReplica {
	pub id: ReplicaID,
	pub path: std::path::PathBuf,
	log_fd: fd::OwnedFd,
	log_len: u32,
	write_cache: VSect,
	id_index: Vec<EventID>,
}

impl LocalReplica {
	pub fn new<R: Rng>(rng: &mut R) -> rustix::io::Result<Self> {
		let id = ReplicaID::new(rng);
		let path_str = format!("/tmp/interlog/{}", id);
		let path = std::path::PathBuf::from(path_str);
		let log_fd = fs::open(
			&path,
			O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC,
			fs::Mode::RUSR | fs::Mode::WUSR,
		)?;
		let log_len = 0;
		let write_cache = VSect::new();
		let id_index = vec![];
		Ok(Self { id, path, log_fd, log_len, write_cache, id_index })
	}

	pub fn write(&mut self, data: &[u8]) -> rustix::io::Result<()> {
		self.write_cache.write(data);
		let bytes_written = self.write_cache.flush(self.log_fd.as_fd())?;
		self.log_len += bytes_written;

		let event_len: EventLen = data.len().into();
		let id = EventID {
			origin: self.id,
			log_offset: self.log_len,
			len: event_len,
		};
		self.id_index.push(id);
		Ok(())
	}

	pub fn read(&self, read_cache: &mut VSect) -> rustix::io::Result<usize> {
		read_cache.fetch(self.log_fd.as_fd(), self.id_index[0].len)
	}
}

fn main() {
	let mut rng = rand::thread_rng();
	let mut replica = LocalReplica::new(&mut rng).expect("failed to open file");
	replica.write(b"Hello, world!\n").expect("failed to write to replica");

	println!("press any key to continue");
	let _ = std::io::stdin().read_exact(&mut []);

	let path = replica.path.clone();
	std::fs::remove_file(path).expect("failed to remove file");
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn read_and_write_to_log() {
		let mut rng = rand::thread_rng();
		let mut replica =
			LocalReplica::new(&mut rng).expect("failed to open file");
		replica.write(b"Hello, world!\n").expect("failed to write to replica");

		let mut read_buf = VSect::new();
		replica.read(&mut read_buf).expect("failed to read to file");
		assert_eq!(&read_buf.as_bytes(), b"Hello, world!\n");
		let path = replica.path.clone();
		std::fs::remove_file(path).expect("failed to remove file");
	}
}
