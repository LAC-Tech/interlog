use alloc::vec::Vec;
use event::Event;
use foldhash::HashMapExt;

/// I am calling this a version vector, out of habit
/// But it says what the seen last seq_n from each address is
/// I suspect it may have a different name..
#[derive(Debug, PartialEq, Eq)]
struct VersionVector(foldhash::HashMap<Address, usize>);

impl VersionVector {
	fn new() -> Self {
		Self(foldhash::HashMap::new())
	}

	fn get(&self, addr: &Address) -> usize {
		self.0.get(addr).copied().unwrap_or(0)
	}

	fn incr(&mut self, addr: Address) {
		*self.0.entry(addr).or_insert(0) += 1;
	}

	fn merge(&mut self, vv: &Self) {
		for (addr, seq_n) in &vv.0 {
			*self.0.entry(*addr).or_insert(0) += seq_n
		}
	}

	fn reset(&mut self) {
		self.0.clear();
	}
}

/// This is two u64s instead of one u128 for alignment in Event Header
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Eq, Hash)]
#[repr(C)]
pub struct Address(pub u64, pub u64);

/// Storage derived state kept in memory
#[derive(Debug, PartialEq, Eq)]
struct Committed {
	vv: VersionVector,
	offsets: Vec<usize>,
}

impl Committed {
	/// Everything in Committed is derived from storage
	fn new<S: ports::Storage>(storage: &S) -> Self {
		let mut vv = VersionVector::new();
		let mut offsets = vec![];

		let mut offset = 0;
		let storage_size = storage.size();
		let mut header_bytes = [0; event::Header::SIZE];

		while storage_size > offset {
			header_bytes.copy_from_slice(
				&storage.as_slice()[offset..event::Header::SIZE],
			);
			offsets.push(offset);

			let header = event::Header::from_bytes(&header_bytes);
			vv.incr(header.id.addr);
			let payload_len: usize = header.payload_len.try_into().unwrap();
			offset += event::stored_size(payload_len);
		}

		// Offsets vectors always have the 'next' offset as last element
		offsets.push(offset);

		Self { offsets, vv }
	}

	// TODO: this will break if I ever start filtering appends
	fn assert_consistent(&self) {
		let vv_sum = self.vv.0.values().sum();
		// assumes every remote event is appended
		let n_events = self.offsets.len() - 1;
		assert_eq!(n_events, vv_sum);
	}
}

/// Operational state that will not be persisted
struct Enqueued {
	offsets: Vec<usize>,
	events: event::Buf,
	vv: VersionVector,
}

impl Enqueued {
	fn new() -> Self {
		// Offsets vectors always have the 'next' offset as last element
		Self {
			offsets: vec![0],
			events: event::Buf::new(),
			vv: VersionVector::new(),
		}
	}
}

/// Commit and Enqueue are defined separately for fine grained control
/// Essentially, they can decide the frequency/pace of writing to disk
pub struct Log<S: ports::Storage> {
	addr: Address,
	enqd: Enqueued,
	cmtd: Committed,
	storage: S,
}

impl<S: ports::Storage> Log<S> {
	pub fn new(addr: Address, storage: S) -> Self {
		let enqd = Enqueued::new();
		let cmtd = Committed::new(&storage);
		Self { addr, enqd, cmtd, storage }
	}

	/// Returns bytes enqueued
	pub fn enqueue(&mut self, payload: &[u8]) -> usize {
		// TODO: this calculation assumes everything cmtd comes from addr
		let seq_n = self.enqd.offsets.len() + self.cmtd.offsets.len() - 2;
		let seq_n = u64::try_from(seq_n).unwrap();
		let id = event::ID { addr: self.addr, seq_n };
		let e = Event { id, payload };

		let curr_offset = *self.enqd.offsets.last().unwrap();
		let next_offset = curr_offset + event::stored_size(payload.len());
		core::assert!(next_offset > curr_offset, "offsets must be monotonic");
		core::assert!(next_offset % 8 == 0, "offsets must be 8 byte aligned");
		self.enqd.offsets.push(next_offset);

		self.enqd.vv.incr(self.addr);

		self.enqd.events.push(e);
		self.enqd.events.as_bytes().len()
	}

	pub fn rollback(&mut self) {
		let last_cmtd_offset = self.cmtd.offsets.last().copied().unwrap();
		self.enqd.offsets.clear();
		self.enqd.offsets.push(last_cmtd_offset);
		self.enqd.events.clear();
		self.enqd.vv.reset();
	}

	/// Returns number of events committed
	pub fn commit(&mut self) -> usize {
		self.cmtd.assert_consistent();

		self.cmtd.vv.merge(&self.enqd.vv);

		let offsets_to_commit = &self.enqd.offsets[1..];
		self.cmtd.offsets.extend(offsets_to_commit);

		let n_events_cmtd = offsets_to_commit.len();

		let last_offset = self.enqd.offsets.last().unwrap();
		let first_offset = self.enqd.offsets.first().unwrap();
		let size = last_offset - first_offset;
		self.storage.append(&self.enqd.events.as_bytes()[..size]);

		self.rollback();

		self.cmtd.assert_consistent();

		n_events_cmtd
	}

	/// Append events coming from a remote log
	/// Intented to be used as part of a sync protocol
	pub fn append_remote<'a>(&mut self, events: event::List<'a>) {
		self.cmtd.assert_consistent();
		let mut last_offset = self.cmtd.offsets.last().copied().unwrap();

		for e in events.iter() {
			self.cmtd.vv.incr(e.id.addr);
			let next_offset = last_offset + event::stored_size(e.payload.len());
			self.cmtd.offsets.push(next_offset);
			last_offset = last_offset;
		}
		self.cmtd.assert_consistent();
		self.storage.append(events.as_bytes());
	}

	pub fn latest(&self, n: usize) -> impl Iterator<Item = Event<'_>> {
		let list = event::List::new(self.storage.as_slice())
		list.iter()
	}

	/*
	pub fn read_from_end(&self, n: usize, buf: &mut event::Buf) {
		let offsets: &[usize] = &self.cmtd.offsets;
		let first = offsets[offsets.len() - 1 - n];
		let last = *offsets.last().unwrap();
		let size = last - first;

		buf.fill(size, |words| self.storage.read(words, first))
	}
	*/

	pub fn stats(&self) -> Stats {
		Stats {
			n_events: self.cmtd.offsets.len() - 1,
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

	/// Given a payload, how much storage space an event containing it will need
	pub fn stored_size(payload_len: usize) -> usize {
		// Ensures 8 byte alignment
		let padded_payload_len = (payload_len + 7) & !7;
		Header::SIZE + padded_payload_len
	}

	pub struct Event<'a> {
		pub id: ID,
		pub payload: &'a [u8],
	}

	impl<'a> Event<'a> {
		fn read(bytes: &'a [u8], offset: usize) -> Event<'a> {
			let header_end = offset + Header::SIZE;
			let header_bytes: &[u8; Header::SIZE] =
				&bytes[offset..header_end].try_into().unwrap();
			let header = Header::from_bytes(header_bytes);

			let payload_end =
				header_end + usize::try_from(header.payload_len).unwrap();

			Self { id: header.id, payload: &bytes[header_end..payload_end] }
		}
	}

	pub struct Buf(Vec<u8>);

	impl Buf {
		pub fn new() -> Self {
			Self(Vec::new())
		}

		pub fn push(&mut self, e: Event<'_>) {
			let new_size = self.0.len() + stored_size(e.payload.len());
			let payload_len = u64::try_from(e.payload.len()).unwrap();
			let header = Header { id: e.id, payload_len };
			self.0.extend(header.as_bytes());
			self.0.extend(e.payload);
			self.0.resize(new_size, 0);
		}

		pub fn as_bytes(&self) -> &[u8] {
			self.0.as_slice()
		}

		// this stupid method make me re-think the whole abstraction
		pub fn clear(&mut self) {
			self.0.clear();
		}

		pub fn iter(&self) -> Iter<'_> {
			Iter { bytes: &self.0, event_index: 0, offset_index: 0 }
		}
	}

	/// Immutable
	pub struct List<'a>(&'a [u8]);

	impl<'a> List<'a> {
		pub fn new(bytes: &'a [u8]) -> Self {
			Self(bytes)
		}
		pub fn iter(&self) -> Iter<'_> {
			Iter { bytes: &self.0, event_index: 0, offset_index: 0 }
		}

		pub fn as_bytes(&self) -> &'a [u8] {
			self.0
		}
	}

	impl<'a> IntoIterator for List<'a> {
		type Item = Event<'a>;
		type IntoIter = Iter<'a>;

		fn into_iter(self) -> Self::IntoIter {
			Iter { bytes: self.0, event_index: 0, offset_index: 0 }
		}
	}

	/// can be produced from anything that produces bytes
	pub struct Iter<'a> {
		bytes: &'a [u8],
		event_index: usize,
		offset_index: usize,
	}

	impl<'a> Iterator for Iter<'a> {
		type Item = Event<'a>;

		fn next(&mut self) -> Option<Self::Item> {
			(self.bytes.len() > self.offset_index).then(|| {
				let e = Event::read(self.bytes, self.offset_index);
				self.event_index += 1;
				self.offset_index += stored_size(e.payload.len());
				e
			})
		}
	}

	#[derive(Clone, Copy)]
	#[cfg_attr(test, derive(PartialEq, Debug))]
	#[repr(C)]
	pub struct ID {
		/// Address of the log this message originated from
		pub addr: Address,
		/// This is the nth event originating from addr
		pub seq_n: u64,
	}

	const _: () = assert!(mem::size_of::<ID>() == 24);

	/// Stand alone, self describing header
	/// All info here is needed to rebuild the log from a binary file.
	#[cfg_attr(test, derive(PartialEq, Debug, Clone, Copy))]
	#[repr(C)]
	pub struct Header {
		pub id: ID,
		/// Length of payload only, ie does not include this header.
		/// This is u64 because usize is not fixed
		pub payload_len: u64,
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
				let seq_n = u.arbitrary()?;
				let payload_len = u.arbitrary()?;
				let id = ID { addr, seq_n };
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
	use arbtest::arbtest;
	use pretty_assertions::assert_eq;
	use test_utils::{jagged_vec::JaggedVec, FaultlessStorage};

	impl<'a> arbitrary::Arbitrary<'a> for Address {
		fn arbitrary(
			u: &mut arbitrary::Unstructured<'a>,
		) -> arbitrary::Result<Self> {
			Ok(Address(u.arbitrary()?, u.arbitrary()?))
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
			log.rollback();

			let post_stats = log.stats();

			assert_eq!(pre_stats, post_stats);
			Ok(())
		});
	}

	#[test]
	fn rebuild_cmtd_in_mem_state_from_storage() {
		arbtest(|u| {
			let storage = FaultlessStorage::new();
			let mut log = Log::new(Address(0, 0), storage);

			for _ in 0..u.arbitrary_len::<usize>()? {
				let bss: JaggedVec<u8> = u.arbitrary()?;

				bss.iter().for_each(|bs| {
					log.enqueue(bs);
				});

				if u.arbitrary()? {
					log.commit();
				} else {
					log.rollback();
				}
			}

			let original = log.cmtd;
			let derived = Log::new(Address(0, 0), log.storage).cmtd;

			assert_eq!(original, derived);

			Ok(())
		});
	}

	/*
	TODO: finish this, use version vector, enforce monotonic seq_n in event buf
	#[test]
	fn receive_remote_events() {
		arbtest(|u| {
			let mut remote_events = event::Buf::new();

			let bss: JaggedVec<u8> = u.arbitrary()?;

			for bs in bss.iter() {
				let id = ID { addr: u.arbitrary(), seq_n };
				bss.iter().for_each(|bs| remote_events.push(bs));
			}

			Ok(())
		});
	}
	*/

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

		let original_cmtd = log.cmtd;
		let rebuilt_cmtd = Log::new(Address(0, 0), log.storage).cmtd;

		assert_eq!(original_cmtd, rebuilt_cmtd);
	}
}
