//! The core of interog.
//!
//! Each log is
//! - single threaded
//! - synchronous
//! - persists to a single append-only fil
//!
//! Core has all the primitives needed to implement sync, but does not call out
//! to the network itself.
//!
//! The following implementation notes may be useful:
//! - Do the dumbest thing I can and test the hell out of it
//! - Allocate all memory at startup
//! - Direct I/O append only file, with KeyIndex that maps ID's to log offsets
//! - Storage engine Works at libc level (rustix), so you can follow man pages.
//! - Assumes linux, 64 bit, little endian - for now at least.
//! - Will orovide hooks to sync in the future, but actual HTTP (or whatever) server is out of scope.

#![cfg_attr(not(test), no_std)]
extern crate alloc;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");
#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

pub mod fixcap;

mod linux;

use core::ops::RangeBounds;
use event::Event;
use fixcap::Vec;

struct Log<'a, S: Storage> {
	addr: Addr,
	enqd: Enqueued<'a>,
	cmtd: Committed<'a, S>,
}

#[derive(Clone, Copy)]
struct Addr {
	word_a: u64,
	word_b: u64,
}

struct Enqueued<'a> {
	offsets: StorageOffsets<'a>,
	events: Vec<'a, u8>,
}

struct Transaction<'a> {
	offsets: &'a [StorageOffset],
	events: &'a [u8],
}

struct Committed<'a, S: Storage> {
	offsets: StorageOffsets<'a>,
	acqs: Acquaintances<'a>,
	storage: S,
}

// TODO:
// "hey lewis… small trick for log buffers… never check for buffer overruns;
// mark a readonly page at the end of the buffer; the OS notifies you with an
// interrupt; result = writes with no checks / error checks i.e. no stalls for
// CPU code pipeline flushes because of branch mispredictions"
// - filasieno
struct ExtAllocMem<'a> {
	cmtd_offsets: &'a mut [StorageOffset],
	cmtd_acqs: &'a mut [Addr],
	enqd_offsets: &'a mut [StorageOffset],
	enqd_events: &'a mut [u8],
}

/// Maps a logical position (nth event) to a byte offset in storage
/// Wrapper around a Vec with some invariants:
/// - always at least one element: next offset, for calculating size
struct StorageOffsets<'a>(
	// This is always one greater than the number of events stored; the last
	// element is the next offset of the next event appended
	Vec<'a, StorageOffset>,
);

/// Q - why bother with with this seperate type?
/// A - because I actually found a bug because when it was just a usize
#[derive(Clone, Copy)]
struct StorageOffset(usize);

/// Addrs the Log has interacted with.
struct Acquaintances<'a>(Vec<'a, Addr>);

/// Where the events are persisted.
/// Written right now so I can simulate faulty storage.
/// Possible concrete implementations:
/// - Disk
/// - In-memory
/// - In-browser (WASM that calls to indexedDB?)
pub trait Storage {
	fn append(&mut self, data: &[u8]);
	fn read(&self, buf: &mut [u8], offset: usize);
}

impl<'a, S: Storage> Log<'a, S> {
	fn new(addr: Addr, storage: S, ext_alloc_mem: ExtAllocMem<'a>) -> Self {
		let enqd = Enqueued {
			offsets: StorageOffsets::new(ext_alloc_mem.enqd_offsets),
			events: Vec::new(ext_alloc_mem.enqd_events),
		};
		let cmtd = Committed {
			offsets: StorageOffsets::new(ext_alloc_mem.cmtd_offsets),
			acqs: Acquaintances::new(ext_alloc_mem.cmtd_acqs),
			storage,
		};
		Self { addr, enqd, cmtd }
	}

	/// Returns bytes enqueued
	fn enqueue(&mut self, payload: &[u8]) -> usize {
		let logical_pos =
			self.enqd.offsets.event_count() + self.cmtd.offsets.event_count();
		let logical_pos = u64::try_from(logical_pos).unwrap();

		let id = event::ID { addr: self.addr, logical_pos };
		let e = Event { id, payload };
		self.enqd.append(&e)
	}

	/// Returns number of events committed
	fn commit(&mut self) -> usize {
		let txn = self.enqd.txn();
		let result = txn.offsets.len();
		self.cmtd.write(txn.offsets, txn.events);
		self.enqd.reset();
		result
	}

	fn rollback(&mut self) {
		self.enqd.reset();
	}

	fn read(
		&self,
		range: impl RangeBounds<usize>,
		buf: &mut event::Buf,
	) -> fixcap::Res {
		self.cmtd.read(range, buf)
	}
}

impl Addr {
	fn new(rand_word_a: u64, rand_word_b: u64) -> Self {
		Self { word_a: rand_word_a, word_b: rand_word_b }
	}
}

impl<'a> Enqueued<'a> {
	/// Returns bytes enqueued
	fn append(&mut self, e: &Event) -> usize {
		self.offsets.update(e);
		e.append_to(&mut self.events);
		self.events.len()
	}

	/// Returns all relevant data to be committed
	fn txn(&'a self) -> Transaction<'a> {
		let offsets = self.offsets.tail();
		let events = &self.events[0..self.offsets.size_spanned()];
		Transaction { offsets, events }
	}

	fn reset(&mut self) {
		self.offsets.reset();
		self.events.clear();
	}
}

impl<'a, S: Storage> Committed<'a, S> {
	fn write(&mut self, offsets: &[StorageOffset], events: &[u8]) {
		self.offsets.extend(offsets);
		self.storage.append(events);
	}

	fn read<R: RangeBounds<usize>>(
		&self,
		range: R,
		buf: &mut event::Buf,
	) -> fixcap::Res {
		buf.clear();

		self.offsets.offset_len_pair(range).map_or(Ok(()), |(offset, len)| {
			buf.fill(len, |words| self.storage.read(words, offset))
		})
	}
}

impl<'a> StorageOffsets<'a> {
	fn new(buf: &'a mut [StorageOffset]) -> Self {
		let mut vec = Vec::new(buf);
		vec.push_unchecked(StorageOffset::new(0));
		Self(vec)
	}

	fn event_count(&self) -> usize {
		self.0.len() - 1
	}

	fn last(&self) -> StorageOffset {
		self.0.last().copied().unwrap()
	}

	fn update(&mut self, e: &Event) {
		let last = self.0.last().copied().unwrap();
		let offset = last.next(e);
		core::assert!(offset.0 > last.0);
		self.0.push_unchecked(offset);
	}

	fn size_spanned(&self) -> usize {
		self.0.last().unwrap().0 - self.0.first().unwrap().0
	}

	fn tail(&self) -> &[StorageOffset] {
		&self.0[1..]
	}

	fn extend(&mut self, other: &[StorageOffset]) {
		self.0.extend_from_slice(&other).unwrap();
	}

	fn reset(&mut self) {
		// By definiton, the last committed event is the first thing in the
		// eqneued buffer before reseting, which happens after committing
		let last_cmtd_event = self.0.first().copied().unwrap();
		self.0.clear();
		self.0.push_unchecked(last_cmtd_event);
	}

	fn offset_len_pair(
		&self,
		range: impl RangeBounds<usize>,
	) -> Option<(usize, usize)> {
		let range = (
			range.start_bound().cloned(),
			range.end_bound().cloned().map(|n| n + 1),
		);

		// TODO: should this stuff be part of StorageOffsets
		let offsets: &[StorageOffset] = &self.0[range];

		offsets
			.first()
			.cloned()
			.zip(offsets.last().cloned())
			.map(|(start, end)| (start.0, end.0 - start.0))
	}
}

impl StorageOffset {
	fn new(n: usize) -> Self {
		// All storage offsets must be 8 byte aligned
		core::assert!(n % 8 == 0);
		Self(n)
	}

	fn next(&self, e: &Event) -> Self {
		Self::new(self.0 + e.stored_size())
	}
}

impl<'a> Acquaintances<'a> {
	fn new(buf: &'a mut [Addr]) -> Self {
		Self(Vec::new(buf))
	}
}

mod event {
	use super::{align_to_8, fixcap, Addr, StorageOffset, Vec};
	use core::iter;
	use core::mem;

	pub struct Event<'a> {
		pub id: ID,
		pub payload: &'a [u8],
	}

	impl<'a> Event<'a> {
		pub fn append_to(&self, byte_vec: &mut Vec<u8>) {
			let header =
				Header { id: self.id, payload_len: self.payload.len() as u64 };

			byte_vec.extend_from_slice_unchecked(header.as_bytes());
			byte_vec.extend_from_slice_unchecked(self.payload);
			byte_vec.resize(align_to_8(byte_vec.len())).unwrap();
		}

		fn read(bytes: &'a [u8], offset: StorageOffset) -> Event<'a> {
			let header_end = offset.0 + Header::SIZE;
			let header_bytes: &[u8; Header::SIZE] =
				&bytes[offset.0..header_end].try_into().unwrap();
			let header = Header::from_bytes(header_bytes);

			let payload_end =
				header_end + usize::try_from(header.payload_len).unwrap();
			let payload = &bytes[header_end..payload_end];

			Self { id: header.id, payload }
		}

		/// How much space it will take in storage, in bytes
		pub fn stored_size(&self) -> usize {
			return Header::SIZE + align_to_8(self.payload.len());
		}
	}

	pub struct Buf<'a> {
		num_events: usize,
		bytes: Vec<'a, u8>,
	}

	impl<'a> Buf<'a> {
		pub fn new(buf: &'a mut [u8]) -> Self {
			Self { num_events: 0, bytes: Vec::new(buf) }
		}

		fn push(&mut self, e: &Event) {
			e.append_to(&mut self.bytes)
		}

		pub fn clear(&mut self) {
			self.num_events = 0;
			self.bytes.clear();
		}

		pub fn fill(
			&mut self,
			len: usize,
			f: impl Fn(&mut [u8]),
		) -> fixcap::Res {
			self.bytes.resize(len)?;
			f(&mut self.bytes);
			Ok(())
		}

		fn as_slice(&self) -> &[u8] {
			&self.bytes
		}

		fn iter(&'a self) -> BufIterator<'a> {
			BufIterator {
				buf: &self,
				event_index: 0,
				offset_index: StorageOffset::new(0),
			}
		}
	}

	pub struct BufIterator<'a> {
		buf: &'a Buf<'a>,
		event_index: usize,
		offset_index: StorageOffset,
	}

	impl<'a> Iterator for BufIterator<'a> {
		type Item = Event<'a>;

		fn next(&mut self) -> Option<Self::Item> {
			if self.event_index == self.buf.num_events {
				return None;
			}

			let e = Event::read(self.buf.as_slice(), self.offset_index);
			self.event_index += 1;
			self.offset_index = self.offset_index.next(&e);
			Some(e)
		}
	}

	#[derive(Clone, Copy)]
	#[repr(C)]
	pub struct ID {
		pub addr: Addr,
		pub logical_pos: u64,
	}

	/// Stand alone, self describing header
	/// All info here is needed to rebuild the log from a binary file.
	#[repr(C)]
	pub struct Header {
		id: ID,
		payload_len: u64,
	}

	impl Header {
		pub const SIZE: usize = mem::size_of::<Self>();

		fn as_bytes(&self) -> &[u8; Self::SIZE] {
			unsafe { mem::transmute(&self) }
		}

		pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> &Self {
			unsafe { mem::transmute(bytes.as_ptr()) }
		}
	}

	const _: () = assert!(mem::size_of::<ID>() == 24);
	const _: () = assert!(Header::SIZE == 32);

	#[cfg(test)]
	mod tests {
		use super::*;
		use pretty_assertions::assert_eq;

		#[test]
		fn lets_write_some_bytes() {
			let mut bytes_buf = [0u8; 127];
			let mut buf = Buf::new(&mut bytes_buf);
			let addr = Addr::new(0, 0);

			let e =
				Event { id: ID { addr, logical_pos: 0 }, payload: b"j;fkls" };

			buf.push(&e);

			assert_eq!(e.payload, buf.iter().next().unwrap().payload);
		}
	}
}

fn align_to_8(n: usize) -> usize {
	(n + 7) & !7
}
