//! Interlog is a distributed, persistent log. It's append only and designed
//! to be a highly available source of truth for many different read models.
//!
//! The following implementation notes may be useful:
//! - Do the dumbest thing I can and test the hell out of it
//! - Allocate all memory at startup
//! - Direct I/O append only file, with KeyIndex that maps ID's to log offsets
//! - Storage engine Works at libc level (rustix), so you can follow man pages.
//! - Assumes linux, 64 bit, little endian - for now at least.
//! - Will orovide hooks to sync in the future, but actual HTTP (or whatever) server is out of scope.
#![cfg_attr(not(test), no_std)]
#[macro_use]
extern crate alloc;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");
#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");
// May work on other OSes but no testing has been done. Remove if you want!
#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

mod event;
mod fixed_capacity;
mod index;
pub mod mem;
mod pervasives;
#[cfg(test)]
mod test_utils;

use crate::index::Index;
pub use crate::pervasives::*;

/// Where the events are persisted.
/// Written right now so I can simulate faulty storage.
/// (Only concrete implementation I can think of is an append only file)
pub trait AppendOnlyStorage {
	fn used(&self) -> StorageQty;
}

pub struct Actor<AOS: AppendOnlyStorage> {
	pub addr: Addr,
	log: log::Log<AOS>,
	index: Index,
}

impl<AOS: AppendOnlyStorage> Actor<AOS> {
	pub fn new(
		addr: Addr,
		txn_size: StorageQty,
		txn_events_per_addr: LogicalQty,
		actual_events_per_addr: LogicalQty,
		aos: AOS,
	) -> Self {
		let log = log::Log::new(txn_size, aos);
		let index = Index::new(txn_events_per_addr, actual_events_per_addr);
		Self { addr, log, index }
	}

	pub fn recv(&mut self, msg: Msg, send: impl Fn(Msg, Addr)) {
		match msg.inner {
			InnerMsg::SyncRes(events) => {
				if let Err(err) = self.write(events) {
					self.index.rollback();
					self.log.rollback();
					let outgoing_msg = Msg {
						inner: InnerMsg::Err(err),
						origin: self.addr,
					};
					send(outgoing_msg, msg.origin);
				}
			}
			InnerMsg::Err(err) => {
				panic!("TODO: implement error logging. {:?}", err)
			}
		}
	}

	fn write<'a, I>(&mut self, events: I) -> Result<(), Err>
	where
		I: IntoIterator<Item = event::Event<'a>>,
	{
		events
			.into_iter()
			.try_for_each(|e| {
				let new_pos = self.log.append(&e).map_err(Err::LogAppend)?;
				self.index.insert(e.id, new_pos).map_err(Err::IndexInsert)
			})
			.and_then(|()| {
				self.index.commit().map_err(Err::IndexCommit)?;
				self.log.commit().map_err(Err::LogCommit)
			})
	}
}

#[derive(Debug)]
pub enum Err {
	LogAppend(fixed_capacity::Overrun),
	IndexInsert(index::InsertErr),
	LogCommit(log::CommitErr),
	IndexCommit(index::CommitErr),
}

pub struct Msg<'a> {
	inner: InnerMsg<'a>,
	origin: Addr,
}

// The 'body' of each message will just be a pointer/
// It must come from some buffer somewhere (ie, TCP buffer)
pub enum InnerMsg<'a> {
	SyncRes(event::Slice<'a>),
	Err(Err),
}

mod log {
	use super::AppendOnlyStorage;
	use crate::event;
	use crate::fixed_capacity;
	use crate::pervasives::*;

	// Can't think of any yet but I am sure there are LOADS
	#[derive(Debug)]
	pub struct CommitErr;

	pub struct Log<AOS: AppendOnlyStorage> {
		txn: event::Buf,
		actual: AOS,
	}

	impl<AOS: AppendOnlyStorage> Log<AOS> {
		pub fn new(txn_size: StorageQty, aos: AOS) -> Self {
			Self { txn: event::Buf::new(txn_size.0), actual: aos }
		}
		pub fn append(
			&mut self,
			e: &event::Event,
		) -> Result<StorageQty, fixed_capacity::Overrun> {
			let result = self.actual.used() + self.txn.used();
			self.txn.push(e)?;
			Ok(result)
		}

		pub fn commit(&mut self) -> Result<(), CommitErr> {
			panic!("TODO")
		}

		pub fn rollback(&mut self) {
			self.txn.clear();
		}
	}
}
/*
use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;

use rand::prelude::*;

use crate::fixed_capacity::Vec;
use crate::index::*;
use crate::pervasives::*;
use mem::Region;

#[derive(Debug)]
pub struct EnqueueErr(fixed_capacity::Overflow);

#[derive(Debug)]
pub enum CommitErr {
	Disk(disk::AppendErr),
	ReadCache(mem::WriteErr),
	TxnWriteBufHasNoEvents,
	KeyIndex(fixed_capacity::Overflow),
}

type WriteRes = Result<(), CommitErr>;

/// A fixed sized structure that caches the latest entries in the log
/// (LIFO caching). The assumption is that things recently added are most
/// likely to be read out again.
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
struct ReadCache {
	mem: Box<[mem::Word]>,
	/// Everything above this is in this cache
	logical_start: usize,
	a: Region,
	b: Region, // pos is always 0 but it's just easier
}

impl fmt::Debug for ReadCache {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ReadCache")
			.field("logical_start", &self.logical_start)
			.field("a", &self.read_a())
			.field("b", &self.read_b())
			.finish()
	}
}

impl ReadCache {
	fn new(capacity: usize) -> Self {
		let mem = vec![0; capacity].into_boxed_slice();
		let logical_start = 0;
		let a = Region::ZERO;
		let b = Region::ZERO; // by definition B always starts at 0
		Self { mem, logical_start, a, b }
	}

	fn extend(
		region: &mut Region,
		dest: &mut [mem::Word],
		src: &[mem::Word],
	) -> Result<(), mem::WriteErr> {
		let extension = Region::new(region.len, src.len());

		extension.write(dest, src)?;
		region.lengthen(src.len());
		Ok(())
	}

	fn overlapping_regions(&self) -> bool {
		self.b.end() > self.a.pos
	}

	fn update(&mut self, es: &[mem::Word]) -> WriteRes {
		let result = match (self.a.empty(), self.b.empty()) {
			(true, true) => {
				self.set_logical_start(es);
				Self::extend(&mut self.a, &mut self.mem, es)
			}
			(false, true) => match Self::extend(&mut self.a, &mut self.mem, es)
			{
				Ok(()) => Ok(()),
				Err(mem::WriteErr) => self.wrap_around(es),
			},
			(_, false) => self.wrap_around(es),
		};

		// Post conditions
		assert_eq!(self.b.pos, 0);
		assert!(!self.overlapping_regions());

		result.map_err(CommitErr::ReadCache)
	}

	fn read(&self, relative_byte_pos: usize) -> Option<event::Event<'_>> {
		let a_bytes = self.read_a();
		let e: Option<_> = event::read(a_bytes, relative_byte_pos);
		if let Some(_) = e {
			return e;
		}

		let relative_byte_pos = relative_byte_pos - a_bytes.len();
		return event::read(self.read_b(), relative_byte_pos);
	}

	fn set_logical_start(&mut self, es: &[mem::Word]) {
		let first_event = event::read(es, 0).expect("no event found at 0");
		self.logical_start = first_event.id.log_pos.0;
	}

	fn wrap_around(&mut self, es: &[mem::Word]) -> Result<(), mem::WriteErr> {
		match self.new_a_pos(es) {
			// Truncate A and write to B
			Some(new_a_pos) => {
				self.a.change_pos(new_a_pos);
				Self::extend(&mut self.b, self.mem.as_mut(), es)
			}
			// We've searched past the end of A and found nothing.
			// B is now A
			None => {
				self.a = Region::new(0, self.b.end());
				self.b = Region::ZERO;
				match Self::extend(&mut self.a, self.mem.as_mut(), es) {
					Ok(()) => Ok(()),
					// the new A cannot fit, erase it.
					// would not occur with buf size 2x or more max event size
					Err(mem::WriteErr) => {
						self.a = Region::ZERO;
						self.set_logical_start(es);
						Self::extend(&mut self.a, self.mem.as_mut(), es)
					}
				}
			}
		}
	}

	fn new_a_pos(&self, es: &[mem::Word]) -> Option<usize> {
		let new_b_end = self.b.end() + es.len();
		event::View::new(self.read_a())
			.scan(0, |offset, e| {
				*offset += e.on_disk_size();
				Some(*offset)
			})
			.find(|&offset| offset > new_b_end)
	}

	fn read_a(&self) -> &[mem::Word] {
		self.a.read(&self.mem).expect("a range to be correct")
	}

	fn read_b(&self) -> &[mem::Word] {
		self.b.read(&self.mem).expect("b range to be correct")
	}
}

#[derive(Debug)]
pub struct Storage {
	disk: disk::Log,
	read_cache: ReadCache,
}

impl Storage {
	fn persist(
		&mut self,
		txn_write_buf: &[mem::Word],
	) -> Result<usize, CommitErr> {
		let bytes_flushed =
			self.disk.append(txn_write_buf).map_err(CommitErr::Disk)?;
		self.read_cache.update(txn_write_buf)?;

		Ok(bytes_flushed)
	}
}

pub struct Config {
	pub read_cache_capacity: usize,
	pub key_index_capacity: usize,
	pub txn_write_buf_capacity: usize,
	pub disk_read_buf_capacity: usize,
}

#[derive(Debug)]
struct Log {
	addr: Addr,
	/// Still counts as "static allocation" as only allocating in constructor
	path: String,
	/// Abstract storage of log, hiding details of any caching
	storage: Storage,
	/// Keeps track of the disk
	byte_len: usize,
	/// The entire index in memory, like bitcask's KeyDir
	/// Maps logical indices to disk offsets
	/// Eventully there will be a map of these, one per origin ID.
	index: Index,
	/// This stores all the events w/headers, contiguously, which means only
	/// one syscall is required to write to disk.
	txn_write_buf: Vec<u8>,
	/// Written to when a value is not in the read_cache
	disk_read_buf: Vec<u8>,
}

pub fn create(dir_path: &str, config: Config) -> rustix::io::Result<Log> {
	let mut rng = rand::thread_rng();
	let addr = Addr::new(rng.gen());
	let path = format!("{dir_path}/{addr}");
	let disk = disk::Log::open(&path)?;
	let read_cache = ReadCache::new(config.read_cache_capacity);

	Ok(Log {
		addr,
		path,
		storage: Storage { disk, read_cache },
		byte_len: 0,
		index: Index::new(config.key_index_capacity),
		txn_write_buf: Vec::new(config.txn_write_buf_capacity),
		disk_read_buf: Vec::new(config.disk_read_buf_capacity),
	})
}

impl Log {
	// Based on FasterLog API
	pub fn enqueue(&mut self, payload: &[u8]) -> Result<(), EnqueueErr> {
		let id = event::ID::new(self.addr, self.index.event_count());
		let e = event::Event { id, payload };

		event::append(&mut self.txn_write_buf, &e).map_err(EnqueueErr)
	}

	// Based on FasterLog API
	pub fn commit(&mut self) -> Result<(), CommitErr> {
		assert_eq!(self.byte_len % 8, 0);

		if event::HEADER_SIZE > self.txn_write_buf.len() {
			return Err(CommitErr::TxnWriteBufHasNoEvents);
		}

		let bytes_flushed = self.storage.persist(&self.txn_write_buf)?;

		// Disk offsets recorded in the Key Index always lag behind by one
		let mut disk_offset = self.byte_len;
		for e in event::View::new(&self.txn_write_buf) {
			self.index.add(disk_offset).map_err(CommitErr::KeyIndex)?;
			disk_offset += e.on_disk_size();
		}

		self.byte_len += bytes_flushed;
		assert!(self.byte_len % 8 == 0);
		Ok(self.txn_write_buf.clear())
	}

	pub fn read(&mut self, logical_pos: usize) -> Option<event::Event<'_>> {
		let byte_cache_start =
			self.index.get(self.storage.read_cache.logical_start)?;

		let byte_pos = self.index.get(logical_pos)?;

		let next_byte_pos = self.index.get(logical_pos + 1)?;

		match byte_pos.checked_sub(byte_cache_start) {
			Some(relative_byte_pos) => {
				// If it's not in here, that means it doesn't exist at all
				self.storage.read_cache.read(relative_byte_pos)
			}
			None => {
				// read from disk
				let len = next_byte_pos
					.checked_sub(byte_pos)
					.expect("key index must always be in sorted order");
				self.disk_read_buf
					.resize(len)
					.expect("disk read buf should fit event");
				self.storage
					.disk
					.read(&mut self.disk_read_buf, byte_pos)
					.expect("reading from disk failed");

				let event = event::read(&self.disk_read_buf, 0)
					.expect("Disk read buf did not contain a valid event");

				Some(event)
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	//use crate::test_utils::*;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;
	use tempfile::TempDir;

	#[test]
	fn log() {
		let tmp_dir = TempDir::with_prefix("interlog-").unwrap();
		let tmp_dir_path = tmp_dir.path().to_string_lossy().into_owned();

		let mut log = create(
			&tmp_dir_path,
			Config {
				read_cache_capacity: 127,
				key_index_capacity: 0x10000,
				txn_write_buf_capacity: 512,
				disk_read_buf_capacity: 256,
			},
		)
		.unwrap();

		log.enqueue(b"I have known the arcane law").unwrap();
		log.commit().unwrap();

		assert_eq!(
			log.read(0).unwrap().payload,
			b"I have known the arcane law"
		);

		log.enqueue(b"On strange roads, such visions met").unwrap();
		log.commit().unwrap();

		dbg!(&log);

		assert_eq!(
			log.read(1).unwrap().payload,
			b"On strange roads, such visions met"
		);
	}

	proptest! {
		// TODO: change stream max to reveal bugs
		#[test]
		fn rw_log(ess in arb_local_events_stream(1, 16, 16)) {
			let tmp_dir = TempDir::with_prefix("interlog-").unwrap();

			let tmp_dir_path =
				tmp_dir
				.path()
				.to_string_lossy().into_owned();

			let mut rng = rand::thread_rng();
			let config = Config {
				txn_write_buf_capacity: 512,
				read_cache_capacity: 16 * 32,
				key_index_capacity: 0x10000,
			};

			let mut log = Log::new(&tmp_dir_path, &mut rng, &config)
				.expect("failed to open file");

			for es in ess {
				for e in es {
					log.enqueue(&e).expect("failed to write to replica");
				}

				log.commit().unwrap();

			}
		}
	}
}
*/
