use alloc::vec::Vec;

use event::Event;

/// This is two u64s instead of one u128 for alignment in Event Header
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Eq, Ord)]
#[repr(C)]
pub struct Address(pub u64, pub u64);

/// Where the events are persisted.
/// Written right now so I can simulate faulty storage.
/// Possible concrete implementations:
/// - Disk
/// - In-memory
/// - In-browser (WASM that calls to indexedDB?)
pub trait Storage {
	fn append(&mut self, data: &[u8]);
	fn read(&self, buf: &mut [u8], offset: usize);
	fn size(&self) -> usize;
}

pub struct Log<S: Storage> {
	pub addr: Address,
	enqd_offsets: Vec<usize>,
	enqd_events: Vec<u8>,
	cmtd_offsets: Vec<usize>,
	storage: S,
}

impl<S: Storage> Log<S> {
	pub fn new(addr: Address, storage: S) -> Self {
		// Offsets vectors always have the 'next' offset as last element
		let (enqd_offsets, cmtd_offsets) = (vec![0], vec![0]);
		let enqd_events = vec![];
		Self { addr, enqd_offsets, enqd_events, cmtd_offsets, storage }
	}

	/// Returns bytes enqueued
	pub fn enqueue(&mut self, payload: &[u8]) -> usize {
		let logical_pos = self.enqd_offsets.len() + self.cmtd_offsets.len() - 2;
		let logical_pos = u64::try_from(logical_pos).unwrap();
		let id = event::ID { addr: self.addr, logical_pos };
		let e = Event { id, payload };

		let curr_offset = *self.enqd_offsets.last().unwrap();
		let next_offset = curr_offset + e.stored_size();
		core::assert!(next_offset > curr_offset, "offsets must be monotonic");
		core::assert!(next_offset % 8 == 0, "offsets must be 8 byte aligned");
		self.enqd_offsets.push(next_offset);

		e.append_to(&mut self.enqd_events);
		self.enqd_events.len()
	}

	/// Returns number of events committed
	pub fn commit(&mut self) -> usize {
		let offsets_to_commit = &self.enqd_offsets[1..];
		self.cmtd_offsets.extend(offsets_to_commit);
		let n_events_cmtd = offsets_to_commit.len();

		let last_offset = self.enqd_offsets.last().unwrap();
		let first_offset = self.enqd_offsets.first().unwrap();
		let size = last_offset - first_offset;
		self.storage.append(&self.enqd_events[..size]);

		self.clear_enqd();
		n_events_cmtd
	}

	pub fn clear_enqd(&mut self) {
		self.enqd_offsets.clear();
		self.enqd_offsets.push(*self.cmtd_offsets.last().unwrap());
		self.enqd_events.clear();
	}

	pub fn read_from_end(&self, n: usize, buf: &mut event::Buf) {
		let offsets: &[usize] = &self.cmtd_offsets;
		let first = offsets[offsets.len() - 1 - n];
		let last = *offsets.last().unwrap();
		let size = last - first;
		buf.fill(n, size, |words| self.storage.read(words, first))
	}

	pub fn stats(&self) -> Stats {
		Stats {
			n_events: self.cmtd_offsets.len() - 1,
			n_bytes: self.storage.size(),
		}
	}
}

#[derive(Debug, PartialEq, Eq)]
pub struct Stats {
	pub n_events: usize,
	pub n_bytes: usize,
}

pub mod event {
	use super::{Address, Vec};
	use core::mem;

	pub struct Event<'a> {
		pub id: ID,
		pub payload: &'a [u8],
	}

	impl<'a> Event<'a> {
		pub fn append_to(&self, byte_vec: &mut Vec<u8>) {
			let new_size = byte_vec.len() + self.stored_size();
			let payload_len = u64::try_from(self.payload.len()).unwrap();
			let header = Header { id: self.id, payload_len };
			byte_vec.extend(header.as_bytes());
			byte_vec.extend(self.payload);
			byte_vec.resize(new_size, 0);
		}

		fn read(bytes: &'a [u8], offset: usize) -> Event<'a> {
			let header_end = offset + Header::SIZE;
			let header_bytes: &[u8; Header::SIZE] =
				&bytes[offset..header_end].try_into().unwrap();
			let header = Header::from_bytes(header_bytes);

			let payload_end =
				header_end + usize::try_from(header.payload_len).unwrap();

			Self { id: header.id, payload: &bytes[header_end..payload_end] }
		}

		/// How much space it will take in storage, in bytes
		/// Ensures 8 byte alignment
		pub fn stored_size(&self) -> usize {
			let padded_payload_len = (self.payload.len() + 7) & !7;
			Header::SIZE + padded_payload_len
		}
	}

	pub struct Buf {
		event_count: usize,
		bytes: Vec<u8>,
	}

	impl Buf {
		pub fn new() -> Self {
			Self { event_count: 0, bytes: Vec::new() }
		}

		pub fn clear(&mut self) {
			self.event_count = 0;
			self.bytes.clear();
		}

		pub fn fill(
			&mut self,
			event_count: usize,
			byte_len: usize,
			read: impl Fn(&mut [u8]),
		) {
			self.bytes.resize(byte_len, 0);
			read(&mut self.bytes);
			self.event_count = event_count;
		}

		fn as_slice(&self) -> &[u8] {
			&self.bytes
		}

		pub fn iter(&self) -> BufIterator<'_> {
			BufIterator { buf: self, event_index: 0, offset_index: 0 }
		}
	}

	pub struct BufIterator<'a> {
		buf: &'a Buf,
		event_index: usize,
		offset_index: usize,
	}

	impl<'a> Iterator for BufIterator<'a> {
		type Item = Event<'a>;

		fn next(&mut self) -> Option<Self::Item> {
			(self.event_index != self.buf.event_count).then(|| {
				let e = Event::read(self.buf.as_slice(), self.offset_index);
				self.event_index += 1;
				self.offset_index += e.stored_size();
				e
			})
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
		use arbtest::arbtest;
		use pretty_assertions::assert_eq;

		// There we go now my transmuting is safe
		#[test]
		fn header_serde() {
			arbtest(|u| {
				let addr = Address(u.arbitrary()?, u.arbitrary()?);
				let logical_pos = u.arbitrary()?;
				let payload_len = u.arbitrary()?;
				let id = ID { addr, logical_pos };
				let expected = Header { id, payload_len };
				let actual = Header::from_bytes(expected.as_bytes());
				assert_eq!(*actual, expected);

				Ok(())
			});
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::test_utils::{jagged_vec::JaggedVec, FaultlessStorage};
	use pretty_assertions::assert_eq;

	use arbitrary::{Result, Unstructured};
	use arbtest::arbtest;

	impl<'a, T> arbitrary::Arbitrary<'a> for JaggedVec<T>
	where
		T: arbitrary::Arbitrary<'a> + Default + Clone + 'a,
		&'a [T]: arbitrary::Arbitrary<'a>,
	{
		fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
			let mut jv = JaggedVec::new();
			let outer_len: usize = u.arbitrary_len::<&[T]>()?;

			for _ in 0..outer_len {
				let inner_len = u.arbitrary_len::<T>()?;
				let iter = u.arbitrary_iter::<T>()?;

				jv.push(std::iter::repeat_n(T::default(), inner_len));
				let buf = jv.last_mut().unwrap();

				for (src, dest) in buf.iter_mut().zip(iter) {
					*src = dest?;
				}
			}

			Ok(jv)
		}
	}

	#[test]
	fn empty_commit() {
		let storage = FaultlessStorage::new();
		let mut log = Log::new(Address(0, 0), storage);
		assert_eq!(log.commit(), 0);
	}

	#[test]
	fn empty_read() {
		arbtest(|u| {
			let storage = FaultlessStorage::new();
			let mut log = Log::new(Address(0, 0), storage);
			let bss: JaggedVec<u8> = u.arbitrary()?;
			bss.iter().for_each(|bs| {
				log.enqueue(bs);
			});
			log.commit();
			let mut buf = event::Buf::new();
			log.read_from_end(0, &mut buf);
			assert!(buf.iter().next().is_none());
			Ok(())
		});
	}

	#[test]
	fn rollbacks_are_atomic() {
		arbtest(|u| {
			let storage = FaultlessStorage::new();
			let mut log = Log::new(Address(0, 0), storage);

			let pre_stats = log.stats();
			assert_eq!(pre_stats, Stats { n_events: 0, n_bytes: 0 });
			let bss: JaggedVec<u8> = u.arbitrary()?;

			bss.iter().for_each(|bs| {
				log.enqueue(bs);
			});
			log.clear_enqd();

			let post_stats = log.stats();

			assert_eq!(pre_stats, post_stats);
			Ok(())
		});
	}

	#[test]
	fn enqueue_commit_and_read_data() {
		let storage = FaultlessStorage::new();
		let mut log = Log::new(Address(0, 0), storage);
		let mut read_buf = event::Buf::new();

		let lyrics: [&[u8]; 4] = [
			b"I have known the arcane law",
			b"On strange roads, such visions met",
			b"That I have no fear, nor concern",
			b"For dangers and obstacles of this world",
		];

		{
			assert_eq!(log.enqueue(lyrics[0]), 64);
			assert_eq!(log.commit(), 1);
			log.read_from_end(1, &mut read_buf);
			let actual = read_buf.iter().next().unwrap().payload;
			assert_eq!(actual, lyrics[0]);
		}

		{
			assert_eq!(log.enqueue(lyrics[1]), 72);
			assert_eq!(log.commit(), 1);
			log.read_from_end(1, &mut read_buf);
			let actual = read_buf.iter().next().unwrap().payload;
			assert_eq!(actual, lyrics[1]);
		}

		// Read multiple things from the buffer
		{
			log.read_from_end(2, &mut read_buf);
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

			log.read_from_end(2, &mut read_buf);
			let mut it = read_buf.iter();
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[2]);
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[3]);
		}
	}
}
