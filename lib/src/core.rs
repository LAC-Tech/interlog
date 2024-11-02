use core::fmt::Write;

use crate::fixcap;
use crate::fixcap::Vec;
use event::Event;

pub struct Log<'a, S: Storage> {
	pub addr: Address,
	enqd_offsets: Vec<'a, StorageOffset>,
	enqd_events: Vec<'a, u8>,
	cmtd_offsets: Vec<'a, StorageOffset>,
	acqs: Acquaintances<'a>,
	storage: S,
}

/// This is two u64s instead of one u128 for alignment in Event Header
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Eq, Ord)]
#[repr(C)]
pub struct Address(pub u64, pub u64);

// TODO:
// "hey lewis… small trick for log buffers… never check for buffer overruns;
// mark a readonly page at the end of the buffer; the OS notifies you with an
// interrupt; result = writes with no checks / error checks i.e. no stalls for
// CPU code pipeline flushes because of branch mispredictions"
// - filasieno
/// Memory provided to the log from the outisde. A log never allocates.
pub struct ExternalMemory<'a> {
	pub cmtd_offsets: &'a mut [StorageOffset],
	pub cmtd_acqs: &'a mut [Address],
	pub enqd_offsets: &'a mut [StorageOffset],
	pub enqd_events: &'a mut [u8],
}

/// Q - why bother with with this seperate type?
/// A - because I actually found a bug because when it was just a usize
#[derive(Clone, Copy, Default)]
pub struct StorageOffset(usize);

/// Addrs the Log has interacted with.
struct Acquaintances<'a>(Vec<'a, Address>);

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
	pub fn new(addr: Address, storage: S, ext_mem: ExternalMemory<'a>) -> Self {
		// Offsets vectors always have the 'next' offset as last element
		let mut enqd_offsets = Vec::new(ext_mem.enqd_offsets);
		enqd_offsets.push_unchecked(StorageOffset::ZERO);
		let mut cmtd_offsets = Vec::new(ext_mem.cmtd_offsets);
		cmtd_offsets.push_unchecked(StorageOffset::ZERO);
		Self {
			addr,
			enqd_offsets,
			enqd_events: Vec::new(ext_mem.enqd_events),
			cmtd_offsets,
			acqs: Acquaintances::new(ext_mem.cmtd_acqs),
			storage,
		}
	}

	/// Returns bytes enqueued
	pub fn enqueue(&mut self, payload: &[u8]) -> usize {
		let logical_pos = u64::try_from(
			self.enqd_offsets.len() + &self.cmtd_offsets.len() - 2,
		)
		.unwrap();

		let id = event::ID { addr: self.addr, logical_pos };
		let e = Event { id, payload };

		{
			let curr = self.enqd_offsets.last().unwrap();
			let next = curr.next(&e);
			core::assert!(next.0 > curr.0);
			self.enqd_offsets.push_unchecked(next);
		}

		e.append_to(&mut self.enqd_events);
		self.enqd_events.len()
	}

	/// Returns number of events committed
	pub fn commit(&mut self) -> usize {
		let size = self.enqd_offsets.last().unwrap().0
			- &self.enqd_offsets.first().unwrap().0;
		let offsets = &self.enqd_offsets[1..];
		let events = &self.enqd_events[0..size];

		let result = offsets.len();

		self.cmtd_offsets.extend_from_slice_unchecked(offsets);
		self.storage.append(events);

		self.clear_enqd();

		result
	}

	// TODO: this functionality is never tested
	pub fn clear_enqd(&mut self) {
		self.enqd_offsets.clear();
		self.enqd_offsets.push_unchecked(*self.cmtd_offsets.last().unwrap());
		self.enqd_events.clear();
	}

	pub fn read_from_end(&self, n: usize, buf: &mut event::Buf) -> fixcap::Res {
		let offsets: &[StorageOffset] = &self.cmtd_offsets;
		let first = offsets[offsets.len() - 1 - n];
		let last = offsets.last().unwrap();
		let size = last.0 - first.0;
		buf.fill(n, size, |words| self.storage.read(words, first.0))
	}
}

impl Address {
	const ZERO: Address = Address(0, 0);
}

impl StorageOffset {
	pub const ZERO: Self = Self(0);

	fn new(n: usize) -> Self {
		// All storage offsets must be 8 byte aligned
		core::assert!(n % 8 == 0);
		Self(n)
	}

	fn next(&self, e: &Event) -> Self {
		Self::new(self.0 + e.stored_size())
	}
}

impl core::fmt::Debug for StorageOffset {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(f, "Offset({})", self.0)
	}
}

impl<'a> Acquaintances<'a> {
	fn new(buf: &'a mut [Address]) -> Self {
		Self(Vec::new(buf))
	}
}

mod event {
	use super::{align_to_8, Address, StorageOffset, Vec};
	use crate::fixcap;
	use core::mem;

	pub struct Event<'a> {
		pub id: ID,
		pub payload: &'a [u8],
	}

	impl<'a> Event<'a> {
		pub fn append_to(&self, byte_vec: &mut Vec<u8>) {
			let header = Header {
				id: self.id,
				payload_len: u64::try_from(self.payload.len()).unwrap(),
			};

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

			Self { id: header.id, payload: &bytes[header_end..payload_end] }
		}

		/// How much space it will take in storage, in bytes
		pub fn stored_size(&self) -> usize {
			Header::SIZE + align_to_8(self.payload.len())
		}
	}

	pub struct Buf<'a> {
		event_count: usize,
		bytes: Vec<'a, u8>,
	}

	impl<'a> Buf<'a> {
		pub fn new(buf: &'a mut [u8]) -> Self {
			Self { event_count: 0, bytes: Vec::new(buf) }
		}

		fn push(&mut self, e: &Event) {
			e.append_to(&mut self.bytes);
			self.event_count += 1;
		}

		pub fn clear(&mut self) {
			self.event_count = 0;
			self.bytes.clear();
		}

		pub fn fill(
			&mut self,
			event_count: usize,
			byte_len: usize,
			f: impl Fn(&mut [u8]),
		) -> fixcap::Res {
			self.bytes.resize(byte_len)?;
			f(&mut self.bytes);
			self.event_count = event_count;
			Ok(())
		}

		fn as_slice(&self) -> &[u8] {
			&self.bytes
		}

		pub fn iter(&'a self) -> BufIterator<'a> {
			BufIterator {
				buf: self,
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
			if self.event_index == self.buf.event_count {
				return None;
			}

			let e = Event::read(self.buf.as_slice(), self.offset_index);
			self.event_index += 1;
			self.offset_index = self.offset_index.next(&e);
			Some(e)
		}
	}

	#[derive(Clone, Copy)]
	#[cfg_attr(test, derive(PartialEq, Debug))]
	#[repr(C)]
	pub struct ID {
		pub addr: Address,
		pub logical_pos: u64,
	}

	const _: () = assert!(mem::size_of::<ID>() == 24);

	/// Stand alone, self describing header
	/// All info here is needed to rebuild the log from a binary file.
	#[cfg_attr(test, derive(PartialEq, Debug, Clone, Copy))]
	#[repr(C)]
	pub struct Header {
		id: ID,
		payload_len: u64,
	}

	impl Header {
		pub const SIZE: usize = mem::size_of::<Self>();

		// SAFETY: Header:
		// - has a fixed C representation
		// - a const time checked size of 32
		// - is memcpyable - has a flat memory structrure with no pointers
		fn as_bytes(&self) -> &[u8; Self::SIZE] {
			unsafe { mem::transmute(self) }
		}

		pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> &Self {
			unsafe { mem::transmute(bytes) }
		}
	}

	const _: () = assert!(Header::SIZE == 32);

	#[cfg(test)]
	mod tests {
		use super::*;
		use pretty_assertions::assert_eq;
		use proptest::prelude::*;

		proptest! {
			// There we go now my transmuting is safe
			#[test]
			fn header_serde(
				rand_word_a in any::<u64>(),
				rand_word_b in any::<u64>(),
				logical_pos: u64,
				payload_len: u64,
			) {
				let addr = Address(rand_word_a, rand_word_b);
				let id = ID {addr, logical_pos};
				let expected = Header { id, payload_len };

				let actual = Header::from_bytes(expected.as_bytes());
				assert_eq!(*actual, expected);
			}
		}

		#[test]
		fn lets_write_some_bytes() {
			let mut bytes_buf = [0u8; 127];
			let mut buf = Buf::new(&mut bytes_buf);
			let addr = Address::ZERO;

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

#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;

	struct TestStorage<'a>(fixcap::Vec<'a, u8>);

	impl<'a> TestStorage<'a> {
		fn new(slice: &'a mut [u8]) -> Self {
			Self(Vec::new(slice))
		}
	}

	impl<'a> Storage for TestStorage<'a> {
		fn append(&mut self, data: &[u8]) {
			self.0.extend_from_slice_unchecked(data)
		}

		fn read(&self, buf: &mut [u8], offset: usize) {
			buf.copy_from_slice(&self.0[offset..offset + buf.len()])
		}
	}

	#[test]
	fn empty_commit() {
		let mut test_storage_buf = [0u8; 256];
		let storage = TestStorage::new(&mut test_storage_buf);
		let ext_mem = ExternalMemory {
			enqd_events: &mut [0u8; 127],
			enqd_offsets: &mut [StorageOffset::ZERO; 64],
			cmtd_offsets: &mut [StorageOffset::ZERO; 64],
			cmtd_acqs: &mut [Address::ZERO; 32],
		};

		let mut log = Log::new(Address::ZERO, storage, ext_mem);
		assert_eq!(log.commit(), 0);
	}

	#[test]
	fn enqueue_commit_and_read_data() {
		let mut test_storage_buf = [0u8; 272];
		let storage = TestStorage::new(&mut test_storage_buf);
		let ext_mem = ExternalMemory {
			enqd_events: &mut [0u8; 136],
			enqd_offsets: &mut [StorageOffset::ZERO; 3],
			cmtd_offsets: &mut [StorageOffset::ZERO; 5],
			cmtd_acqs: &mut [Address::ZERO; 1],
		};

		let mut log = Log::new(Address::ZERO, storage, ext_mem);
		let mut read_buf = [0u8; 136];
		let mut read_buf = event::Buf::new(&mut read_buf);

		let lyrics: [&[u8]; 4] = [
			b"I have known the arcane law",
			b"On strange roads, such visions met",
			b"That I have no fear, nor concern",
			b"For dangers and obstacles of this world",
		];

		{
			assert_eq!(log.enqueue(lyrics[0]), 64);
			assert_eq!(log.commit(), 1);
			log.read_from_end(1, &mut read_buf).unwrap();
			let actual = read_buf.iter().next().unwrap().payload;
			assert_eq!(actual, lyrics[0]);
		}

		{
			assert_eq!(log.enqueue(lyrics[1]), 72);
			assert_eq!(log.commit(), 1);
			log.read_from_end(1, &mut read_buf).unwrap();
			let actual = read_buf.iter().next().unwrap().payload;
			assert_eq!(actual, lyrics[1]);
		}

		// Read multiple things from the buffer
		{
			log.read_from_end(2, &mut read_buf).unwrap();
			let mut it = read_buf.iter();
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[0]);
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[1]);
		}

		// Bulk commit two things
		{
			assert_eq!(log.enqueue(lyrics[2]), 64);
			assert_eq!(log.enqueue(lyrics[3]), 136);
			assert_eq!(log.commit(), 2);

			log.read_from_end(2, &mut read_buf).unwrap();
			let mut it = read_buf.iter();
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[2]);
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[3]);
		}
	}
}
