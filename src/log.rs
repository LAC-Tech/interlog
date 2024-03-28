//! User facing functions for readidng and writing to interlog replicas.
//! This can be considered the "top level" of the library.

use alloc::boxed::Box;
use alloc::string::String;

use crate::fixvec;
use crate::fixvec::FixVec;
use crate::log_id::LogID;
use crate::region;
use crate::{disk, event};
use event::Event;
use region::Region;

#[derive(Debug)]
pub enum WriteErr {
	Disk(disk::AppendErr),
	ReadCache(region::WriteErr),
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

	pub fn extend(
		region: &mut Region,
		dest: &mut [u8],
		src: &[u8]
	) -> Result<(), region::WriteErr> {
		let extension = Region::new(region.len, src.len());

		extension.write(dest, src)?;
		region.lengthen(src.len());
		Ok(())
	}

	pub fn update(&mut self, es: &[u8]) -> WriteRes {
		let result = match (self.a.empty(), self.b.empty()) {
			(true, true) => {
				self.set_logical_start(es);
				Self::extend(&mut self.a, &mut self.mem, es)
			}
			(false, true) => match Self::extend(&mut self.a, &mut self.mem, es)
			{
				Ok(()) => Ok(()),
				Err(region::WriteErr) => self.wrap_around(es)
			},
			(_, false) => self.wrap_around(es)
		};

		result.map_err(WriteErr::ReadCache)
	}

	fn set_logical_start(&mut self, es: &[u8]) {
		self.logical_start =
			event::read(es, 0).expect("event at 0").id.logical_pos;
	}

	fn wrap_around(&mut self, es: &[u8]) -> Result<(), region::WriteErr> {
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
					Err(region::WriteErr) => {
						self.a = Region::ZERO;
						self.set_logical_start(es);
						Self::extend(&mut self.a, self.mem.as_mut(), es)
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

struct Config {
	read_cache_capacity: usize,
	key_index_capacity: usize,
	txn_write_buf_capacity: usize
}

struct Log {
	id: LogID,
	/// Still counts as "static allocation" as only allocating in constructor
	path: String,
	disk: disk::Log,
	/// Keeps track of the disk
	byte_len: usize,
	read_cache: ReadCache,
	/// The entire index in memory, like bitcask's KeyDir
	/// Maps logical indices to disk offsets
	key_index: FixVec<usize>,
	/// This stores all the events w/headers, contiguously, which means only
	/// one syscall is required to write to disk.
	txn_write_buf: FixVec<u8>
}

impl Log {
	fn new<R: rand::Rng>(
		dir_path: &str,
		rng: &mut R,
		config: &Config
	) -> rustix::io::Result<Self> {
		let id = LogID::new(rng);
		let path = format!("{dir_path}/{id}");

		disk::Log::open(&path).map(|disk| Self {
			id,
			path,
			disk,
			byte_len: 0,
			read_cache: ReadCache::new(config.read_cache_capacity),
			key_index: FixVec::new(config.key_index_capacity),
			txn_write_buf: FixVec::new(config.txn_write_buf_capacity)
		})
	}

	pub fn enqueue(&mut self, payload: &[u8]) -> fixvec::Res {
		let logical_pos = self.key_index.len();
		let e =
			Event { id: event::ID { origin: self.id, logical_pos }, payload };

		event::append(&mut self.txn_write_buf, &e)
	}

	fn commit(&mut self) -> Result<(), WriteErr> {
		let bytes_flushed =
			self.disk.append(&self.txn_write_buf).map_err(WriteErr::Disk)?;

		// TODO: the below operations need to be made atomic w/ each other
		self.txn_write_buf.clear();
		self.read_cache.update(&self.txn_write_buf)?;

		// Disk offsets recored in the Key Index always lag behind by one
		let mut disk_offset = self.byte_len;

		for e in event::View::new(&self.txn_write_buf) {
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
				.expect("failed to open temp file")
				.path()
				.to_string_lossy().into_owned();

			let mut rng = rand::thread_rng();
			let config = Config {
				txn_write_buf_capacity: 512,
				read_cache_capacity: 16 * 32,
				key_index_capacity: 0x10000,
			};

			let mut log = Log::new(&tmp_dir, &mut rng, &config)
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
