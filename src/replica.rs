//! User facing functions for readidng and writing to interlog replicas.
//! This can be considered the "top level" of the library.
use crate::replica_id::ReplicaID;
use crate::unit;
use crate::util::{FixVec, FixVecOverflow, Segment};
use crate::{disk, event};
use rand::prelude::*;
use rustix::io; // TODO make OpenErr in disk, and return that, remove this

// Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
pub const MAX_SIZE: usize = 2048 * 1024;

#[derive(Debug)]
pub enum WriteErr {
	Disk(disk::AppendErr),
	ReadCache(FixVecOverflow),
	WriteCache(FixVecOverflow),
	KeyIndex(FixVecOverflow)
}

type WriteRes = Result<(), WriteErr>;

#[derive(Debug)]
pub enum ReadErr {
	KeyIndex,
	ClientBuf(FixVecOverflow)
}

type ReadRes = Result<(), ReadErr>;

/// For various reasons (monitoring, performance, debugging) it will be helpful
/// to report some statistics. How much of a buffer is being used, lengths, etc
trait StatsReporter {
	type Stats;
	fn stats(&self) -> Self::Stats;
}

/*
struct KeyIndex(FixVec<unit::Byte>);

/// Maps logical indices to disk offsets
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
		pos: unit::Logical
	) -> impl Iterator<Item = unit::Byte> + '_ {
		self.0.iter().skip(pos.into()).copied()
	}
}
*/

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
/// ```text
/// ┌---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┐
/// | A | A | B | B | B |   | X | X | X | Y | Z | Z | Z | Z |   |   |
/// └---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┘
/// ```
///
/// The top segment contains events A and B, while the bottom segment contains
/// X, Y and Z
/// As more events are added, they will be appended after B, overwriting the
/// bottom segment, til it wraps round again.
pub struct ReadCache {
	/// The entire index in memory, like bitcask's KeyDir
	key_index: FixVec<unit::Byte>,
	cache_start: unit::Logical,
	mem: Box<[u8]>,
	a: Segment,
	b_end: usize
}

impl ReadCache {
	pub fn new(config: ReadCacheConfig) -> Self {
		Self {
			key_index: FixVec::new(config.key_index_capacity.into()),
			cache_start: unit::Logical(0),
			mem: vec![0; config.mem_capacity.into()].into_boxed_slice(),
			a: Segment::new(0, 0),
			b_end: 0
		}
	}

	pub fn update(&mut self, es: &event::Buf) {
		let len: usize = es.byte_len().into();

		let a_would_overflow = self.a.end + len > self.mem.len();

		if !a_would_overflow && self.b_end == 0 {
			self.write_a(es);
			return;
		}

		let a_will_be_modified = self.b_end + len >= self.a.pos;

		if !a_will_be_modified {
			self.write_b(es);
			return;
		}

		let b_would_overflow = self.b_end + len > self.mem.len();

		if b_would_overflow {
			self.a = Segment::new(0, self.b_end);
			self.b_end = 0;
		}

		let new_b_end = self.b_end + len;

		let new_a_pos = event::EventIntoIterator::new(self.read_a())
			.scan(unit::Byte(0), |offset, e| {
				*offset += e.on_disk_size();
				Some(*offset)
			})
			.find(|&offset| offset > new_b_end.into());

		match new_a_pos {
			// Truncate A and write to B
			Some(new_a_pos) => {
				self.a.change_pos(new_a_pos.into());
				self.write_b(es);
			}
			// We've searched past the end of A and found nothing.
			// B is now A
			None => {
				self.a = Segment::new(0, self.b_end);
				self.b_end = 0;
				self.write_a(es);
			}
		}
	}

	fn write_a(&mut self, es: &event::Buf) {
		let diff: usize = es.byte_len().into();
		self.mem[self.a.len..self.a.len + diff].copy_from_slice(es.as_bytes());
		self.a.lengthen(diff);
	}

	fn write_b(&mut self, es: &event::Buf) {
		let diff: usize = es.byte_len().into();
		self.mem[self.b_end..self.b_end + diff].copy_from_slice(es.as_bytes());
		self.b_end += diff;
	}

	fn read_a(&self) -> &[u8] {
		&self.mem[self.a.range()]
	}

	fn read_b(&self) -> &[u8] {
		&self.mem[0..self.b_end]
	}
}

/// Read and write data bus for data going into and out of the replica
/// These buffers are written to independently, but the idea of putting them
/// together is a scenario where there are many IO buses to one Log
///
/// Both buffers represent 1..N events, with the following invariants:
/// INVARIANTS
/// - starts at the start of an event
/// - ends at the end of an event
/// - aligns events to 8 bytes
pub mod io_bus {
	use super::{ReadErr, ReadRes, StatsReporter, WriteErr, WriteRes};
	use crate::event;
	use crate::replica_id::ReplicaID;
	use crate::unit;

	pub struct IOBus {
		/// This stores all the events w/headers, contigously, which means only
		/// one syscall is required to write to disk.
		txn_write_buf: event::Buf,
		read_buf: event::Buf
	}

	impl IOBus {
		pub fn new(
			txn_write_buf_capacity: unit::Byte,
			read_buf_capacity: unit::Byte
		) -> Self {
			Self {
				txn_write_buf: event::Buf::new(txn_write_buf_capacity),
				read_buf: event::Buf::new(read_buf_capacity)
			}
		}

		pub fn write<'a, I>(
			&mut self,
			from: unit::Logical,
			replica_id: ReplicaID,
			new_events: I
		) -> WriteRes
		where
			I: IntoIterator<Item = &'a [u8]>
		{
			self.txn_write_buf.clear();

			// Write Events with their headers
			for (i, val) in new_events.into_iter().enumerate() {
				let pos = from + i.into();
				let id = event::ID { origin: replica_id, pos };
				let e = event::Event { id, payload: val };
				self.txn_write_buf.append(&e).map_err(WriteErr::WriteCache)?;
			}

			Ok(())
		}

		pub fn txn_write_buf(&self) -> &event::Buf {
			&self.txn_write_buf
		}

		pub fn read_in<'a, I>(&mut self, events: I) -> ReadRes
		where
			I: IntoIterator<Item = event::Event<'a>>
		{
			for e in events {
				self.read_buf.append(&e).map_err(ReadErr::ClientBuf)?;
			}

			Ok(())
		}

		pub fn read_out(&self) -> impl Iterator<Item = event::Event<'_>> {
			self.read_buf.into_iter()
		}
	}

	pub struct Stats {
		txn_write_buf_len: unit::Byte
	}

	impl StatsReporter for IOBus {
		type Stats = Stats;
		fn stats(&self) -> Stats {
			Stats { txn_write_buf_len: self.txn_write_buf.byte_len() }
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
					&bus.txn_write_buf.read(0).is_none(),
					"should contain no event");

				let vals = es.iter().map(Deref::deref);
				bus.write(0.into(), replica_id, vals)
					.expect("buf should have enough");

				// Post conditions
				let actual: Vec<_> = bus.txn_write_buf.into_iter()
					.map(|e| e.payload)
					.collect();
				assert_eq!(&actual, &es);
			}
		}
	}
}

struct Log {
	disk: disk::Log,
	byte_len: unit::Byte,
	read_cache: ReadCache
}

impl Log {
	fn persist(&mut self, txn_write_buf: &event::Buf) -> Result<(), WriteErr> {
		let bytes_flushed = self
			.disk
			.append(txn_write_buf.as_bytes())
			.map(unit::Byte::align)
			.map_err(WriteErr::Disk)?;

		// TODO: the below operations need to be made atomic w/ each other
		self.read_cache.update(txn_write_buf)?;

		// Disk offsets recored in the Key Index always lag behind by one
		let mut disk_offset = self.byte_len;

		for e in self.read_cache.into_iter() {
			self.key_index.push(disk_offset)?;
			disk_offset += e.on_disk_size();
		}

		self.byte_len += bytes_flushed;

		Ok(())
	}

	fn logical_len(&self) -> unit::Logical {
		self.key_index.len()
	}

	fn byte_len(&self) -> unit::Byte {
		self.byte_len
	}

	fn events_since(
		&self,
		pos: unit::Logical
	) -> impl Iterator<Item = event::Event<'_>> {
		let byte_offsets = self.key_index.read_since(pos);
		self.read_cache.read(byte_offsets)
	}
}

struct ReadCacheConfig {
	key_index_capacity: unit::Logical,
	mem_capacity: unit::Byte
}

/// A replica on the same machine as user code
pub struct Local {
	pub id: ReplicaID,
	pub path: std::path::PathBuf,
	log: Log
}

// TODO: store data larger than read cache
impl Local {
	pub fn new<R: Rng>(
		dir_path: &std::path::Path,
		rng: &mut R,
		config: ReadCacheConfig
	) -> io::Result<Self> {
		let id = ReplicaID::new(rng);

		let path = dir_path.join(id.to_string());

		let log = Log {
			disk: disk::Log::open(&path)?,
			// TODO: this assumes the log is empty
			byte_len: 0.into(),
			read_cache: ReadCache::new(config)
		};

		Ok(Self { id, path, log })
	}

	// Event local to the replica, that don't yet have an ID
	pub fn local_write<'b, I>(
		&mut self,
		io_bus: &mut io_bus::IOBus,
		datums: I
	) -> Result<(), WriteErr>
	where
		I: IntoIterator<Item = &'b [u8]>
	{
		io_bus.write(self.log.logical_len(), self.id, datums)?;
		self.log.persist(io_bus.txn_write_buf())
	}

	// TODO: assumes cache is 1:1 with disk
	pub fn read<P: Into<unit::Logical>>(
		&mut self,
		io_bus: &mut io_bus::IOBus,
		pos: P
	) -> Result<(), ReadErr> {
		let events = self.log.events_since(pos.into());
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
			let config = ReadCacheConfig {
				key_index_capacity: unit::Logical(128),
				mem_capacity: unit::Byte(1024),
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
					.map(|e| e.payload.to_vec())
					.collect();

				assert_eq!(events, es);
			}
		}
	}
}
