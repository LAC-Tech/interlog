//! User facing functions for readidng and writing to interlog replicas.
//! This can be considered the "top level" of the library.

use core::ops::Range;

use crate::fixvec;
use crate::fixvec::FixVec;
use crate::log_id::LogID;
use crate::util::region;
use crate::{disk, event};
use region::Region;

// Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
pub const MAX_SIZE: usize = 2048 * 1024;

#[derive(Debug)]
pub enum WriteErr {
	Disk(disk::AppendErr),
	ReadCache(region::ExtendOverflow),
	TxnWriteBuf(fixvec::Overflow),
	KeyIndex(fixvec::Overflow)
}

type WriteRes = Result<(), WriteErr>;

#[derive(Debug)]
pub enum ReadErr {
	KeyIndex,
	ClientBuf(fixvec::Overflow)
}

type ReadRes = Result<(), ReadErr>;

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
	mem: Box<[u8]>,
	/// Everything above this is in this cache
	pub logical_start: usize,
	a: Region,
	b: Region // pos is always 0 but it's just easier
}

impl ReadCache {
	pub fn new(capacity: usize) -> Self {
		let mem = vec![0; capacity].into_boxed_slice();
		let logical_start = 0;
		let a = Region::ZERO;
		let b = Region::ZERO; // by definition B always starts at 0
		Self { mem, logical_start, a, b }
	}

	pub fn update(&mut self, es: &[u8]) -> WriteRes {
		let result = match (self.a.empty(), self.b.empty()) {
			(true, true) => {
				self.set_logical_start(es);
				self.a.extend(&mut self.mem, es)
			}
			(false, true) => match self.a.extend(&mut self.mem, es) {
				Ok(()) => Ok(()),
				Err(region::ExtendOverflow) => self.wrap_around(es)
			},
			(_, false) => self.wrap_around(es)
		};

		result.map_err(WriteErr::ReadCache)
	}

	fn set_logical_start(&mut self, es: &[u8]) {
		self.logical_start =
			event::read(es, 0).expect("event at 0").id.logical_pos;
	}

	fn wrap_around(&mut self, es: &[u8]) -> Result<(), region::ExtendOverflow> {
		match self.new_a_pos(es) {
			// Truncate A and write to B
			Some(new_a_pos) => {
				self.a.change_pos(new_a_pos);
				self.b.extend(self.mem.as_mut(), es)
			}
			// We've searched past the end of A and found nothing.
			// B is now A
			None => {
				self.a = Region::new(0, self.b.end());
				self.b = Region::ZERO;
				match self.a.extend(self.mem.as_mut(), es) {
					Ok(()) => Ok(()),
					// the new A cannot fit, erase it.
					// would not occur with buf size 2x or more max event size
					Err(region::ExtendOverflow) => {
						self.a = Region::ZERO;
						self.set_logical_start(es);
						self.a.extend(&mut self.mem, es)
					}
				}
			}
		}
	}

	fn new_a_pos(&self, es: &[u8]) -> Option<usize> {
		let new_b_end = self.b.end() + es.len();
		event::View::new(self.read_a())
			.scan(0, |offset, e| {
				*offset += e.on_disk_size();
				Some(*offset)
			})
			.find(|&offset| offset > new_b_end)
	}

	fn read_a(&self) -> &[u8] {
		self.a.read(&self.mem).expect("a range to be correct")
	}

	fn read_b(&self) -> &[u8] {
		self.b.read(&self.mem).expect("b range to be correct")
	}
}

struct TxnWriteBuf(FixVec<u8>);

impl TxnWriteBuf {
	fn new(capacity: usize) -> Self {
		Self(FixVec::new(capacity))
	}

	fn write<'a, I>(
		&mut self,
		logical_from: usize,
		origin: LogID,
		new_events: I
	) -> WriteRes
	where
		I: IntoIterator<Item = &'a [u8]>
	{
		self.0.clear();

		// Write Events with their headers
		for (i, payload) in new_events.into_iter().enumerate() {
			let id = event::ID { origin, logical_pos: logical_from + i };
			let e = event::Event { id, payload };
			event::append(&mut self.0, &e).map_err(WriteErr::TxnWriteBuf)?;
		}

		Ok(())
	}

	fn iter(&self) -> impl Iterator<Item = event::Event<'_>> {
		event::View::new(&self.0)
	}
}

impl AsRef<[u8]> for TxnWriteBuf {
	fn as_ref(&self) -> &[u8] {
		&self.0
	}
}

impl AsMut<[u8]> for TxnWriteBuf {
	fn as_mut(&mut self) -> &mut [u8] {
		&mut self.0
	}
}

struct ReadBuf(FixVec<u8>);

impl ReadBuf {
	fn new(capacity: usize) -> Self {
		Self(FixVec::new(capacity))
	}

	pub fn read_in<'a, I>(&mut self, events: I) -> ReadRes
	where
		I: IntoIterator<Item = event::Event<'a>>
	{
		for e in events {
			event::append(&mut self.0, &e).map_err(ReadErr::ClientBuf)?;
		}

		Ok(())
	}

	pub fn read_out(&self) -> impl Iterator<Item = event::Event<'_>> {
		event::View::new(&self.0)
	}
}

struct Log {
	disk: disk::Log,
	/// Keeps track of the disk
	byte_len: usize,
	read_cache: ReadCache,
	/// The entire index in memory, like bitcask's KeyDir
	/// Maps logical indices to disk offsets
	key_index: FixVec<usize>
}

impl Log {
	fn persist(&mut self, txn_write_buf: &TxnWriteBuf) -> Result<(), WriteErr> {
		let bytes_flushed =
			self.disk.append(txn_write_buf).map_err(WriteErr::Disk)?;

		// TODO: the below operations need to be made atomic w/ each other
		self.read_cache.update(txn_write_buf.as_ref())?;

		// Disk offsets recored in the Key Index always lag behind by one
		let mut disk_offset = self.byte_len;

		for e in txn_write_buf.iter() {
			self.key_index.push(disk_offset).map_err(WriteErr::KeyIndex)?;
			disk_offset += e.on_disk_size();
		}

		self.byte_len += bytes_flushed;

		assert!(
			self.byte_len % 8 == 0,
			"length of log in bytes should be aligned to 8 bytes, given {}",
			self.byte_len
		);

		Ok(())
	}
}
/*
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
	use super::*;
	use crate::unit;
	use crate::util::FixVec;

	pub struct IOBus {
		/// This stores all the events w/headers, contigously, which means only
		/// one syscall is required to write to disk.
		pub txn_write_buf: TxnWriteBuf,
		pub read_buf: FixVec<u8>
	}

	impl IOBus {
		pub fn new(
			txn_write_buf_capacity: unit::Byte,
			read_buf_capacity: unit::Byte
		) -> Self {
			Self {
				txn_write_buf: TxnWriteBuf::new(txn_write_buf_capacity),
				read_buf: FixVec::new(read_buf_capacity.into())
			}
		}
	}

	#[cfg(test)]
	mod tests {
		use super::*;
		use crate::replica_id::ReplicaID;
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
					event::read(&bus.txn_write_buf, 0).is_none(),
					"should contain no event");

				let vals = es.iter().map(Deref::deref);
				bus.txn_write_buf.write(0.into(), replica_id, vals)
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

/// A replica on the same machine as user code
pub struct Local {
	pub id: ReplicaID,
	pub path: std::path::PathBuf,
	key_index: KeyIndex,
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

		let key_index = KeyIndex::new(config.key_index_capacity);

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
		io_bus.txn_write_buf.write(self.log.logical_len(), self.id, datums)?;
		self.log.persist(&io_bus.txn_write_buf)
	}

	// TODO: assumes cache is 1:1 with disk
	pub fn read<P: Into<unit::Logical>>(
		&mut self,
		io_bus: &mut io_bus::IOBus,
		pos: P
	) -> Result<(), ReadErr> {
		let events = self.log.events_since(pos.into());
		io_bus.read_buf.read_in(events)
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
*/
