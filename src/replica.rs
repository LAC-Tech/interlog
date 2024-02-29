use fs::OFlags;

use crate::replica_id::ReplicaID;
use crate::unit;
use crate::util::{
	CircBuf, CircBufWrapAround, FixVec, FixVecOverflow, Segment,
};
use crate::{disk, event};
use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};

// Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
pub const MAX_SIZE: usize = 2048 * 1024;

type O = OFlags;

#[derive(Debug)]
pub enum WriteErr {
	Disk(disk::Err),
	ReadCache(FixVecOverflow),
	WriteCache(FixVecOverflow),
	KeyIndex(FixVecOverflow),
}

type WriteRes = Result<(), WriteErr>;

#[derive(Debug)]
pub enum ReadErr {
	KeyIndex,
	ClientBuf(FixVecOverflow),
}

type ReadRes = Result<(), ReadErr>;

/// For various reasons (monitoring, performance, debugging) it will be helpful
/// to report some statistics. How much of a buffer is being used, lengths, etc
trait StatsReporter {
	type Stats;
	fn stats(&self) -> Self::Stats;
}

struct KeyIndex(FixVec<unit::Byte>);

impl KeyIndex {
	fn new(capacity: usize) -> Self {
		Self(FixVec::new(capacity))
	}

	fn len(&self) -> unit::Logical {
		self.0.len().into()
	}

	fn push(&mut self, byte_offset: unit::Byte) -> WriteRes {
		self.0.push(byte_offset).map_err(WriteErr::KeyIndex)
	}

	fn get(&self, logical_pos: unit::Logical) -> Result<unit::Byte, ReadErr> {
		let i: usize = logical_pos.into();
		self.0.get(i).cloned().ok_or(ReadErr::KeyIndex)
	}

	fn read_since(
		&self,
		pos: unit::Logical,
	) -> impl Iterator<Item = unit::Byte> + '_ {
		self.0.iter().skip(pos.into()).copied()
	}
}

/// ReadCache is a fixed sized structure that caches the latest entries in the
/// log (LIFO caching). The assumption is that things recently added are
/// most likely to be read out again.
///
/// To do this I'm using a single circular buffer, with two "write pointers".
/// At any given point in time the buffer will have two contiguous segments,
/// populated with events.
///
/// The reason to keep the two segments contiguous is so they can be easily
/// memcpy'd. So no event is split by the circular buffer.
///
/// There is a Top Segment, and a Bottom Segment.
///
/// We start with a top segment. The bottom segement gets created when we
/// reach the end of the circular buffer and need to wrap around, eating into
/// the former top segment.
///
/// Example:
///
/// ┌---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┐
/// | A | A | B | B | B |   | X | X | X | Y | Z | Z | Z | Z |   |   |
/// └---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┘
///
/// The top segment contains events A and B, while the bottom segment contains
/// X, Y and Z
/// As more events are added, they will be appended after B, overwriting the
/// bottom segment, til it wraps round again.

struct ReadCache {
	buf: FixVec<u8>,
	head: ReadCacheWritePtr,
	tail: Option<ReadCacheWritePtr>,
}

struct ReadCacheWritePtr {
	disk_offset: unit::Byte,
	slice: Segment,
}

impl ReadCache {
	fn new(capacity: unit::Byte) -> Self {
		let buf = FixVec::new(capacity.into());
		let head = ReadCacheWritePtr {
			disk_offset: 0.into(),
			slice: Segment::new(0, 0),
		};
		let tail = None;
		Self { buf, head, tail }
	}

	#[inline]
	fn capacity(&self) -> usize {
		self.buf.capacity()
	}

	fn update(&mut self, write_cache: &[u8]) -> WriteRes {
		// TODO: if this overflows start from the start of the buffer
		// if it overflows twice, bail
		if let Err(_) = self.buf.extend_from_slice(write_cache) {
			self.head.disk_offset += self.head.slice.len.into();
			self.head.slice = Segment::new(0, write_cache.len());
		}

		//self.top.disk_offset += write_cache.len().into();
		Ok(())
	}

	// TODO: read first contiguous slice, then the next one
	fn read<O>(&self, disk_offsets: O) -> impl Iterator<Item = event::Event<'_>>
	where
		O: Iterator<Item = unit::Byte>,
	{
		disk_offsets
			.map(|offset| {
				event::read(&self.buf, offset - self.head.disk_offset)
			})
			.fuse()
			.flatten()
	}
}

/// Read and write data bus for data going into and out of the replica
/// These buffers are written to independently, but the idea of putting them
/// together is a scenario where there are many IO buses to one Log
mod io_bus {
	use super::{ReadErr, ReadRes, StatsReporter, WriteErr, WriteRes};
	use crate::event;
	use crate::replica_id::ReplicaID;
	use crate::unit;
	use crate::util::FixVec;

	pub struct IOBus {
		/// This stores all the events w/headers, contigously, which means only
		/// one syscall is required to write to disk.
		txn_write_buf: FixVec<u8>,
		read_buf: FixVec<u8>,
	}

	impl IOBus {
		pub fn new(
			txn_write_buf_capacity: unit::Byte,
			read_buf_capacity: unit::Byte,
		) -> Self {
			Self {
				txn_write_buf: FixVec::new(txn_write_buf_capacity.into()),
				read_buf: FixVec::new(read_buf_capacity.into()),
			}
		}

		pub fn write<'a, I>(
			&mut self,
			from: unit::Logical,
			replica_id: ReplicaID,
			new_events: I,
		) -> WriteRes
		where
			I: IntoIterator<Item = &'a [u8]>,
		{
			self.txn_write_buf.clear();

			// Write Events with their headers
			for (i, val) in new_events.into_iter().enumerate() {
				let pos = from + i.into();
				let id = event::ID { origin: replica_id, pos };
				let e = event::Event { id, val };
				self.txn_write_buf
					.append_event(&e)
					.map_err(WriteErr::WriteCache)?;
			}

			Ok(())
		}

		pub fn txn_write_buf(&self) -> &[u8] {
			&self.txn_write_buf
		}

		pub fn read_in<'a, I>(&mut self, events: I) -> ReadRes
		where
			I: IntoIterator<Item = event::Event<'a>>,
		{
			for e in events {
				self.read_buf.append_event(&e).map_err(ReadErr::ClientBuf)?;
			}

			Ok(())
		}

		pub fn read_out(&self) -> impl Iterator<Item = event::Event<'_>> {
			self.read_buf.into_iter()
		}
	}

	pub struct Stats {
		txn_write_buf_len: unit::Byte,
	}
	impl StatsReporter for IOBus {
		type Stats = Stats;
		fn stats(&self) -> Stats {
			Stats { txn_write_buf_len: self.txn_write_buf.len().into() }
		}
	}

	#[cfg(test)]
	mod tests {
		use super::*;
		use crate::test_utils::*;
		use core::ops::Deref;
		use pretty_assertions::assert_eq;
		use proptest::prelude::*;

		proptest! {
			/*
			#[test]
			fn combine_buffers(
				es1 in arb_local_events(16, 16),
				es2 in arb_local_events(16, 16)
			) {
				// Setup
				let mut rng = rand::thread_rng();
				let replica_id = ReplicaID::new(&mut rng);

				let mut buf1 = FixVec::new(0x800);
				let mut buf2 = FixVec::new(0x800);

				let start: unit::Logical = 0.into();

				buf1.append_local_events(start, replica_id, es1.iter().map(Deref::deref))
					.expect("buf should have enough");

				buf2.append_local_events(start, replica_id, es2.iter().map(Deref::deref))
					.expect("buf should have enough");

				buf1.extend_from_slice(&buf2).expect("buf should have enough");

				let actual: Vec<_> = buf1.into_iter().map(|e| e.val).collect();

				let mut expected = Vec::new();
				expected.extend(&es1);
				expected.extend(&es2);

				assert_eq!(&actual, &expected);
			}
			*/
			#[test]
			fn w_many_events(es in arb_local_events(16, 16)) {
				// Setup
				let mut rng = rand::thread_rng();
				let replica_id = ReplicaID::new(&mut rng);

				let mut bus = IOBus::new(0x400.into(), 0x400.into());

				// Pre conditions
				assert_eq!(
					bus.stats().txn_write_buf_len,
					0.into(), "buf should start empty");
				assert!(
					event::read(&bus.txn_write_buf, 0).is_none(),
					"should contain no event");

				let vals = es.iter().map(Deref::deref);
				bus.write(0.into(), replica_id, vals)
					.expect("buf should have enough");

				// Post conditions
				let actual: Vec<_> = bus.txn_write_buf.into_iter()
					.map(|e| e.val)
					.collect();
				assert_eq!(&actual, &es);
			}
		}
	}
}

pub struct Config {
	pub index_capacity: usize,
	pub read_cache_capacity: unit::Byte,
}

// ASSUMPTIONS:
// single writer, multiple readers
pub struct Local {
	pub id: ReplicaID,
	pub path: std::path::PathBuf,
	log_fd: fd::OwnedFd,
	log_len: unit::Byte,
	read_cache: ReadCache,
	// The entire index in memory, like bitcask's KeyDir
	key_index: KeyIndex,
}

// TODO: store data larger than read cache
impl Local {
	pub fn new<R: Rng>(
		dir_path: &std::path::Path,
		rng: &mut R,
		config: Config,
	) -> io::Result<Self> {
		let id = ReplicaID::new(rng);

		let path = dir_path.join(id.to_string());
		let flags = O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC;
		let mode = fs::Mode::RUSR | fs::Mode::WUSR;
		let log_fd = fs::open(&path, flags, mode)?;

		let log_len: unit::Byte = 0.into();
		let read_cache = ReadCache::new(config.read_cache_capacity);
		let key_index = KeyIndex::new(config.index_capacity);

		Ok(Self { id, path, log_fd, log_len, read_cache, key_index })
	}

	fn persist(&mut self, txn_write_buf: &[u8]) -> Result<(), WriteErr> {
		let fd = self.log_fd.as_fd();
		let bytes_flushed = disk::write(fd, txn_write_buf)
			.map(unit::Byte::align)
			.map_err(WriteErr::Disk)?;

		// TODO: the below operations need to be made atomic w/ each other
		self.read_cache.update(txn_write_buf)?;
		let mut byte_offset = self.log_len;

		// TODO: event iterator implemented DIRECTLY on the read cache
		// TODO: THIS ASSUMES THE READ CACHE STARTED EMPTY
		for e in self.read_cache.buf.into_iter() {
			self.key_index.push(byte_offset)?;
			byte_offset += e.on_disk_size();
		}

		dbg!(byte_offset);
		dbg!(self.log_len);
		dbg!(bytes_flushed);
		assert_eq!(byte_offset - self.log_len, bytes_flushed);

		self.log_len += byte_offset;

		Ok(())
	}

	// Event local to the replica, that don't yet have an ID
	pub fn local_write<'b, I>(
		&mut self,
		io_bus: &mut io_bus::IOBus,
		datums: I,
	) -> Result<(), WriteErr>
	where
		I: IntoIterator<Item = &'b [u8]>,
	{
		io_bus.write(self.key_index.len(), self.id, datums)?;
		self.persist(io_bus.txn_write_buf())
	}

	// TODO: assumes cache is 1:1 with disk
	pub fn read<P: Into<unit::Logical>>(
		&mut self,
		io_bus: &mut io_bus::IOBus,
		pos: P,
	) -> Result<(), ReadErr> {
		let byte_offsets = self.key_index.read_since(pos.into());
		let events = self.read_cache.read(byte_offsets);
		io_bus.read_in(events)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::test_utils::*;
	use core::ops::Deref;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;
	use tempfile::TempDir;

	proptest! {
		// TODO: change stream max to reveal bugs
		#[test]
		fn rw_log(ess in arb_local_events_stream(1, 16, 16)) {
			let tmp_dir = TempDir::with_prefix("interlog-")
				.expect("failed to open temp file");

			let mut rng = rand::thread_rng();
			let config = Config {
				index_capacity: 128,
				read_cache_capacity: unit::Byte(1024),
			};

			let mut replica = Local::new(tmp_dir.path(), &mut rng, config)
				.expect("failed to open file");

			let mut client =
				io_bus::IOBus::new(unit::Byte(0x400), unit::Byte(0x400));

			for es in ess {
				let vals = es.iter().map(Deref::deref);
				replica.local_write(&mut client, vals)
					.expect("failed to write to replica");

				replica.read(&mut client, 0).expect("failed to read to file");

				let events: Vec<_> = client.read_out()
					.map(|e| e.val.to_vec())
					.collect();

				assert_eq!(events, es);
			}
		}
	}
}
