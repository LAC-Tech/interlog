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
mod log;
pub mod mem;
mod pervasives;
pub mod storage;
#[cfg(test)]
mod test_utils;

/*
pub use log::*;
pub use pervasives::*;
*/

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

struct ExtAllocMem<'a> {
	cmtd_offsets: &'a mut [StorageOffset],
	cmtd_acqs: &'a mut [Addr],
	enqd_offsets: &'a mut [StorageOffset],
	enqd_events: &'a mut [u8],
}

struct StorageOffsets<'a>(Vec<'a, StorageOffset>);
#[derive(Clone, Copy)]
struct StorageOffset(usize);

struct Acquaintances<'a>(Vec<'a, Addr>);

trait Storage {
	// TODO: return type with all the wonderful things that can go wrong
	fn append(&mut self, bytes: &[u8]);
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
		let logical_pos = logical_pos as u64;

		let id = event::ID { addr: self.addr, logical_pos };
		let e = event::Event { id, payload };
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
}

impl<'a> Enqueued<'a> {
	fn append(&mut self, e: &event::Event) -> usize {
		self.offsets.update(e);
		e.append_to(&mut self.events);
		self.events.len()
	}

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
}

impl<'a> StorageOffsets<'a> {
	fn new(buf: &'a mut [StorageOffset]) -> Self {
		let mut vec = Vec::new(buf);
		vec.push(StorageOffset::new(0));
		Self(vec)
	}

	fn event_count(&self) -> usize {
		self.0.len() - 1
	}

	fn last(&self) -> StorageOffset {
		self.0.last().copied().unwrap()
	}

	fn update(&mut self, e: &event::Event) {
		let last = self.0.last().copied().unwrap();
		let offset = last.next(e);
		core::assert!(offset.0 > last.0);
		self.0.push(offset).unwrap();
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
		let last_cmtd_event = self.0.first().copied().unwrap();
		self.0.clear();
		self.0.push(last_cmtd_event);
	}
}

impl StorageOffset {
	fn new(n: usize) -> Self {
		core::assert!(n % 8 == 0);
		Self(n)
	}

	fn next(&self, e: &event::Event) -> Self {
		let size = event::Header::SIZE + e.payload.len();
		Self::new(self.0 + align_to_8(size))
	}
}

impl<'a> Acquaintances<'a> {
	fn new(buf: &'a mut [Addr]) -> Self {
		Self(Vec::new(buf))
	}
}

mod event {
	use super::{align_to_8, Addr, StorageOffset, Vec};
	use core::mem;
	pub struct Event<'a> {
		pub id: ID,
		pub payload: &'a [u8],
	}

	impl<'a> Event<'a> {
		pub fn append_to(&self, byte_vec: &mut Vec<u8>) {
			let header =
				Header { id: self.id, payload_len: self.payload.len() as u64 };
			let header_bytes: &[u8; 32] = unsafe { mem::transmute(&header) };

			byte_vec.extend_from_slice(header_bytes);
			byte_vec.extend_from_slice(self.payload);
			byte_vec.resize(align_to_8(byte_vec.len()));
		}

		fn read(bytes: &'a [u8], offset: StorageOffset) -> Event<'a> {
			let header_end = offset.0 + Header::SIZE;
			let header_bytes: &[u8] = &bytes[offset.0..header_end];
			let header: &Header =
				unsafe { mem::transmute(header_bytes.as_ptr()) };

			let payload_end = header_end + header.payload_len as usize;
			let payload = &bytes[header_end..payload_end];

			Self { id: header.id, payload }
		}
	}

	#[derive(Clone, Copy)]
	#[repr(C)]
	pub struct ID {
		pub addr: Addr,
		pub logical_pos: u64,
	}

	#[repr(C)]
	pub struct Header {
		id: ID,
		payload_len: u64,
	}

	impl Header {
		pub const SIZE: usize = mem::size_of::<Header>();
	}
}

fn align_to_8(n: usize) -> usize {
	(n + 7) & !7
}
