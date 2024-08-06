//! The core of interlog.
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
#[cfg(test)]
mod test_utils;

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

/*
struct LogIterator<'a, S: Storage> {
	storage: S,
	byte_index: usize,
	// TODO probably won't need this later as storage will have allocated cache
	_marker: core::marker::PhantomData<&'a ()>,
}

impl<'a, S: Storage> Iterator for LogIterator<'a, S> {
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		let header = {
			// TODO: core::array::try_from_fn is in nightly
			let mut header_bytes = [0u8; event::Header::SIZE];
			for i in 0..event::Header::SIZE {
				header_bytes[i] = self.storage.read(self.byte_index)?;
			}
			event::Header::from_bytes(&header_bytes);
		}

		let payload = {
			let mut payload = [0u8; ]
		}

		// Get Payload
		panic!("TODO: collect Header, then collect Payload");
	}
}
impl<'a, S: Storage> IntoIterator for Log<'a, S> {
	type Item = Event<'a>;

	fn into_iter(self) -> Self::IntoIter {
		self.data.into_iter()
	}
}
*/
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
		let range = (
			range.start_bound().cloned(),
			range.end_bound().cloned().map(|n| n + 1),
		);

		let offsets: &[StorageOffset] = &self.offsets.0[range];

		let offsets = offsets
			.first()
			.cloned()
			.zip(offsets.last().cloned())
			.map(|(start, end)| (start.0, end.0 - start.0));

		offsets.map_or(Ok(()), |(offset, len)| {
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
		fn push(&mut self, e: Event) {
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
}

fn align_to_8(n: usize) -> usize {
	(n + 7) & !7
}

#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;

	#[test]
	fn lets_write_some_bytes() {
		/*
		const bytes_buf = try testing.allocator.alloc(u8, 127);
		defer testing.allocator.free(bytes_buf);
		var buf = Event.Buf.init(bytes_buf);

		const seed: u64 = std.crypto.random.int(u64);
		var rng = std.Random.Pcg.init(seed);
		const id = Addr.init(std.Random.Pcg, &rng);

		const evt = Event{
			.id = .{ .origin = id, .logical_pos = 0 },
			.payload = "j;fkls",
		};

		buf.append(&evt);
		var it = buf.iter();

		while (it.next()) |e| {
			try testing.expectEqualSlices(u8, evt.payload, e.payload);
		}

		const actual = buf.read(StorageOffset.zero);

		try testing.expectEqualDeep(actual, evt);
		*/
	}
}
