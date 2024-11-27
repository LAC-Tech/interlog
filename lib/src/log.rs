use alloc::vec::Vec;
use event::Event;
use foldhash::HashMapExt;

pub trait LogicalClock {
	fn get(&self, addr: &Address) -> u64;
}

/// I am calling this a version vector, out of habit
/// But it says what the seen last seq_n from each address is
/// I suspect it may have a different name..
/// u64 for value, not usize, because it will be sent across network
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct VersionVector(foldhash::HashMap<Address, u64>);

impl VersionVector {
	fn new() -> Self {
		Self(foldhash::HashMap::new())
	}

	fn set(&mut self, addr: Address, disk_offset: u64) {
		self.0
			.entry(addr)
			.and_modify(|current| {
				if disk_offset <= *current {
					panic!(
							"Version Vectors must monotonically increase: cannot replace ({}) with ({})",
							disk_offset, *current
						);
				}
				*current = disk_offset;
			})
			.or_insert(disk_offset);
	}
}

impl LogicalClock for VersionVector {
	fn get(&self, addr: &Address) -> u64 {
		self.0.get(addr).copied().unwrap_or(0)
	}
}

/// This is two u64s instead of one u128 for alignment in Event Header
#[derive(Clone, Copy, Default, PartialEq, PartialOrd, Eq, Hash)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[repr(C)]
pub struct Address(pub u64, pub u64);

impl core::fmt::Debug for Address {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(f, "Address({:016X}{:016X})", self.0, self.1)
	}
}

/// Storage derived state kept in memory
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
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
		let events = event::Iter::new(storage.read());

		for Event { id, payload } in events {
			offsets.push(offset);
			vv.set(id.addr, id.disk_offset);
			offset += event::stored_size(payload);
		}

		// Offsets vectors always have the 'next' offset as last element
		offsets.push(offset);

		Self { offsets, vv }
	}
}

/// Operational state that will not be persisted
struct Enqueued {
	offsets: Vec<usize>,
	events: event::Buf,
}

impl Enqueued {
	fn new() -> Self {
		// Offsets vectors always have the 'next' offset as last element
		Self { offsets: vec![0], events: event::Buf::new() }
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

impl<Storage: ports::Storage> Log<Storage> {
	pub fn new(addr: Address, storage: Storage) -> Self {
		let enqd = Enqueued::new();
		let cmtd = Committed::new(&storage);
		Self { addr, enqd, cmtd, storage }
	}

	/// Returns bytes enqueued
	pub fn enqueue(&mut self, payload: &[u8]) -> usize {
		let disk_offset = self.enqd.offsets.last().copied().unwrap();

		let disk_offset = u64::try_from(disk_offset).unwrap();
		let id = event::ID { addr: self.addr, disk_offset };
		let e = Event { id, payload };

		let curr_offset = *self.enqd.offsets.last().unwrap();
		let next_offset = curr_offset + event::stored_size(payload);
		core::assert!(next_offset > curr_offset, "offsets must be monotonic");
		core::assert!(next_offset % 8 == 0, "offsets must be 8 byte aligned");
		self.enqd.offsets.push(next_offset);

		self.enqd.events.append(e);

		self.enqd.events.as_bytes().len()
	}

	pub fn rollback(&mut self) {
		let last_cmtd_offset = self.cmtd.offsets.last().copied().unwrap();
		self.enqd.offsets.clear();
		self.enqd.offsets.push(last_cmtd_offset);
		self.enqd.events.clear();
	}

	/// Returns number of events committed
	pub fn commit(&mut self) -> Result<u64, Storage::Err> {
		let offsets_to_commit = &self.enqd.offsets[1..];
		self.cmtd.offsets.extend(offsets_to_commit);

		let n_events_cmtd: u64 = offsets_to_commit.len().try_into().unwrap();
		if let Some(&highest_offset) =
			&self.enqd.offsets.get(self.enqd.offsets.len().wrapping_sub(2))
		{
			self.cmtd.vv.set(self.addr, highest_offset.try_into().unwrap());
		}

		self.storage.append(self.enqd.events.as_bytes())?;

		self.rollback();

		Ok(n_events_cmtd)
	}

	/*
	/// Append events coming from a remote log
	/// Intented to be used as part of a sync protocol
	// TODO: test
	pub fn append_remote(&mut self, events: event::Slice<'_>) {
		self.cmtd.assert_consistent();
		let mut next_offset = self.cmtd.offsets.last().copied().unwrap();

		for e in events.iter() {
			self.cmtd.vv.incr(e.id.addr, 1);
			next_offset += event::stored_size(e.payload);
			self.cmtd.offsets.push(next_offset);
		}

		self.cmtd.assert_consistent();
		self.storage.append(events.as_bytes());
	}
	*/

	pub fn latest(&self, n: usize) -> impl Iterator<Item = Event<'_>> {
		let offsets = &self.cmtd.offsets;

		let events = offsets
			.get(offsets.len() - n - 1) // Offsets always include one extra
			.map(|&offset| &self.storage.read()[offset..])
			.unwrap_or(&[]);

		event::Iter::new(events)
	}

	// SYNC PROTOCOL /////////////////////////////////////////////////////////

	pub fn logical_clock(&self) -> &impl LogicalClock {
		&self.cmtd.vv
	}

	/*
	pub fn events_since(
		&self,
		lc: &impl LogicalClock,
	) -> impl Iterator<Item = Event<'_>> {
		panic!("TODO: Implement me");
	}

	/// Append events coming from a remote log
	// TODO: test
	pub fn append_remote(&mut self, events: event::Slice<'_>) {
		self.cmtd.assert_consistent();
		let mut next_offset = self.cmtd.offsets.last().copied().unwrap();

		for e in events.iter() {
			self.cmtd.vv.incr(e.id.addr, 1);
			next_offset += event::stored_size(e.payload);
			self.cmtd.offsets.push(next_offset);
		}

		self.cmtd.assert_consistent();
		self.storage.append(events.as_bytes());
	}
	*/

	///////////////////////////////////////////////////////////////////////////

	pub fn stats(&self) -> Stats {
		Stats {
			n_events: self.cmtd.offsets.len() - 1,
			n_bytes: self.storage.size(),
		}
	}
}

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub struct Stats {
	pub n_events: usize,
	pub n_bytes: usize,
}

pub mod event {
	use super::Address;
	use alloc::vec::Vec;
	use core::mem;

	/// Given a payload, how much storage space an event containing it will need
	pub fn stored_size(payload: &[u8]) -> usize {
		let payload_len = payload.len();
		// Ensures 8 byte alignment
		let padded_payload_len = (payload_len + 7) & !7;
		Header::SIZE + padded_payload_len
	}

	#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
	pub struct Event<'a> {
		pub id: ID,
		pub payload: &'a [u8],
	}

	pub struct Buf(Vec<u8>);

	impl Buf {
		pub fn new() -> Self {
			Buf(vec![])
		}

		pub fn append(&mut self, Event { id, payload }: Event) {
			let new_size = self.0.len() + stored_size(payload);
			let payload_len = u64::try_from(payload.len()).unwrap();
			let header = Header { id, payload_len };
			self.0.extend(header.as_bytes());
			self.0.extend(payload);
			self.0.resize(new_size, 0);
		}

		pub fn as_bytes(&self) -> &[u8] {
			&self.0
		}

		pub fn clear(&mut self) {
			self.0.clear()
		}
	}

	impl<'a> FromIterator<Event<'a>> for Buf {
		fn from_iter<T: IntoIterator<Item = Event<'a>>>(iter: T) -> Self {
			iter.into_iter().fold(Buf::new(), |mut buf, e| {
				buf.append(e);
				buf
			})
		}
	}

	/// Immutable
	pub struct Slice<'a>(&'a [u8]);

	impl<'a> Slice<'a> {
		pub fn new(bytes: &'a [u8]) -> Self {
			Self(bytes)
		}

		pub fn iter(&self) -> Iter<'_> {
			Iter::new(self.0)
		}

		pub fn as_bytes(&self) -> &'a [u8] {
			self.0
		}
	}

	impl<'a> IntoIterator for Slice<'a> {
		type Item = Event<'a>;
		type IntoIter = Iter<'a>;

		fn into_iter(self) -> Self::IntoIter {
			Iter::new(self.0)
		}
	}

	/// can be produced from anything that produces bytes
	#[derive(Debug)]
	pub struct Iter<'a> {
		bytes: &'a [u8],
		event_index: usize,
		offset: usize,
		header_buf: [u8; Header::SIZE],
	}

	impl<'a> Iter<'a> {
		pub fn new(bytes: &'a [u8]) -> Self {
			Self {
				bytes,
				event_index: 0,
				offset: 0,
				header_buf: [0u8; Header::SIZE],
			}
		}
	}

	impl<'a> Iterator for Iter<'a> {
		type Item = Event<'a>;

		fn next(&mut self) -> Option<Self::Item> {
			if self.offset >= self.bytes.len() {
				return None;
			}

			let header_end = self.offset + Header::SIZE;
			let header = &self.bytes[self.offset..header_end];
			self.header_buf.copy_from_slice(header);
			let header = Header::from_bytes(&self.header_buf);

			let payload_end =
				header_end + usize::try_from(header.payload_len).unwrap();

			let e = Event {
				id: header.id,
				payload: &self.bytes[header_end..payload_end],
			};
			self.event_index += 1;
			self.offset += stored_size(e.payload);
			Some(e)
		}
	}

	#[derive(Clone, Copy)]
	#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
	#[repr(C)]
	pub struct ID {
		/// Address of the log this message originated from
		pub addr: Address,
		/// This is the nth event originating from addr
		pub disk_offset: u64,
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
		pub fn as_bytes(&self) -> &[u8; Self::SIZE] {
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
				let disk_offset = u.arbitrary()?;
				let payload_len = u.arbitrary()?;
				let id = ID { addr, disk_offset };
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
	use crate::linux;
	use arbtest::arbtest;
	use pretty_assertions::assert_eq;
	use test_utils::jagged_vec::JaggedVec;

	fn temp_mmap_storage() -> linux::MmapStorage {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("log.bin");
		linux::MmapStorage::new(file_path, 0xFFFFFF).unwrap()
	}

	#[test]
	fn empty_commit() {
		let storage = temp_mmap_storage();
		let mut log = Log::new(Address(0, 0), storage);
		assert_eq!(log.commit(), Ok(0));
	}

	#[test]
	fn empty_read() {
		arbtest(|u| {
			let storage = temp_mmap_storage();
			let mut log = Log::new(u.arbitrary()?, storage);
			let bss: JaggedVec<u8> = u.arbitrary()?;
			for bs in bss.iter() {
				log.enqueue(bs);
			}
			log.commit().unwrap();

			assert!(log.latest(0).next().is_none());
			Ok(())
		});
	}

	#[test]
	fn rollbacks_are_atomic() {
		arbtest(|u| {
			let storage = temp_mmap_storage();
			let mut log = Log::new(u.arbitrary()?, storage);

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
			let storage = temp_mmap_storage();
			let addr = u.arbitrary()?;
			let mut log = Log::new(addr, storage);

			for _ in 0..u.arbitrary_len::<usize>()? {
				let bss: JaggedVec<u8> = u.arbitrary()?;

				bss.iter().for_each(|bs| {
					log.enqueue(bs);
				});

				if u.arbitrary()? {
					log.commit().unwrap();
				} else {
					log.rollback();
				}
			}

			let original = log.cmtd;
			let derived = Log::new(addr, log.storage).cmtd;

			assert_eq!(original, derived);

			Ok(())
		});
	}

	#[test]
	fn enqueue_commit_and_read_data() {
		let addr = Address(0, 0);
		let storage = temp_mmap_storage();
		let mut log = Log::new(addr, storage);

		let lyrics: [&[u8]; 4] = [
			b"I have known the arcane law",
			b"On strange roads, such visions met",
			b"That I have no fear, nor concern",
			b"For dangers and obstacles of this world",
		];

		{
			assert_eq!(log.enqueue(lyrics[0]), 64);
			assert_eq!(log.commit(), Ok(1));
			let actual: Vec<Event> = log.latest(1).collect();

			let expected = vec![Event {
				id: event::ID { addr, disk_offset: 0 },
				payload: lyrics[0],
			}];

			assert_eq!(actual, expected);
		}

		// Read multiple things from the buffer
		{
			assert_eq!(log.enqueue(lyrics[1]), 72);
			assert_eq!(log.commit(), Ok(1));
			let actual: Vec<Event> = log.latest(2).collect();
			let expected = vec![
				Event {
					id: event::ID { addr, disk_offset: 0 },
					payload: lyrics[0],
				},
				Event {
					id: event::ID { addr, disk_offset: 64 },
					payload: lyrics[1],
				},
			];
			assert_eq!(actual, expected);
		}

		// Bulk commit two things
		{
			assert_eq!(log.enqueue(lyrics[2]), 64);
			assert_eq!(log.enqueue(lyrics[3]), 136);
			assert_eq!(log.commit(), Ok(2));

			let actual: Vec<Event> = log.latest(2).collect();
			let expected = vec![
				Event {
					id: event::ID { addr, disk_offset: 136 },
					payload: lyrics[2],
				},
				Event {
					id: event::ID { addr, disk_offset: 200 },
					payload: lyrics[3],
				},
			];
			assert_eq!(actual, expected);
		}

		let original_cmtd = log.cmtd;
		let rebuilt_cmtd = Log::new(Address(0, 0), log.storage).cmtd;

		assert_eq!(original_cmtd, rebuilt_cmtd);
	}
}
