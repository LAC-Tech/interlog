use crate::replica_id::ReplicaID;
use crate::unit;
use crate::util::{FixVec, FixVecOverflow, FixVecRes, Segment, Segmentable};

// TODO: do I need to construct this oustide of this module?
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
pub struct ID {
	pub origin: ReplicaID,
	pub pos: unit::Logical,
}

impl ID {
	pub fn new<P: Into<unit::Logical>>(origin: ReplicaID, disk_pos: P) -> Self {
		ID { origin, pos: disk_pos.into() }
	}
}

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
#[repr(C)]
pub struct Header {
	byte_len: usize,
	id: ID,
}

impl Header {
	const SIZE: usize = std::mem::size_of::<Self>();

	pub fn range(start: unit::Byte) -> Segment {
		Segment::new(start.0, Self::SIZE)
	}

	pub fn new(byte_len: usize, id: ID) -> Self {
		Self { byte_len, id }
	}
}

#[derive(Clone, Debug)]
pub struct Event<'a> {
	pub id: ID,
	pub val: &'a [u8],
}

impl<'a> Event<'a> {
	pub fn on_disk_size(&self) -> unit::Byte {
		let size: unit::Byte = (Header::SIZE + self.val.len()).into();
		size.align()
	}
}

/// 1..N events backed by a fixed capacity byte buffer
///
/// INVARIANTS
/// - starts at the start of an event
/// - ends at the end of an event
/// - aligns events to 8 bytes
impl FixVec<u8> {
	pub fn append_event(&mut self, event: &Event) -> FixVecRes {
		let Event { id, val } = *event;
		let offset = self.len().into();
		let header_segment = Header::range(offset);
		let byte_len = val.len();
		let val_segment = header_segment.next(byte_len);
		let new_len = unit::Byte(val_segment.end).align();
		self.resize(new_len.into(), 0)?;

		let header = Header { byte_len, id };
		let header = bytemuck::bytes_of(&header);

		self[header_segment.range()].copy_from_slice(header);
		self[val_segment.range()].copy_from_slice(val);

		Ok(())
	}

	/*
	pub fn append_local_events<'a, I>(
		&mut self,
		start: unit::Logical,
		origin: ReplicaID,
		vals: I,
	) -> FixVecRes
	where
		I: IntoIterator<Item = &'a [u8]>,
	{
		for (i, val) in vals.into_iter().enumerate() {
			let pos = start + i.into();
			let id = ID { origin, pos };
			let e = Event { id, val };
			self.append_event(&e)?;
		}

		Ok(())
	}

	pub fn append_events<'a, I>(
		&mut self,
		events: I,
	) -> Result<(), FixVecOverflow>
	where
		I: IntoIterator<Item = Event<'a>>,
	{
		for e in events {
			self.append_event(&e)?;
		}

		Ok(())
	}
	*/
}

pub fn read<B, O>(bytes: &B, offset: O) -> Option<Event<'_>>
where
	B: Segmentable<u8>,
	O: Into<unit::Byte>,
{
	let header_segment = Header::range(offset.into());
	let header_bytes = bytes.segment(&header_segment)?;
	let &Header { id, byte_len } = bytemuck::from_bytes(header_bytes);
	let val_segment = header_segment.next(byte_len);
	let val = bytes.segment(&val_segment)?;
	Some(Event { id, val })
}

pub struct BufIntoIterator<'a> {
	event_buf: &'a FixVec<u8>,
	index: unit::Byte,
}

impl<'a> Iterator for BufIntoIterator<'a> {
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		let result = read(self.event_buf, self.index)?;
		self.index += result.on_disk_size();
		Some(result)
	}
}

impl<'a> IntoIterator for &'a FixVec<u8> {
	type Item = Event<'a>;
	type IntoIter = BufIntoIterator<'a>;

	fn into_iter(self) -> Self::IntoIter {
		BufIntoIterator { event_buf: self, index: 0.into() }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::test_utils::*;
	use core::ops::Deref;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;

	proptest! {
		#[test]
		fn rw_single_event(
			e in prop::collection::vec(any::<u8>(), 0..=8)
		) {
			// Setup
			let mut rng = rand::thread_rng();
			let mut buf = FixVec::new(256);
			let replica_id = ReplicaID::new(&mut rng);
			let event = Event {id: ID::new(replica_id, 0), val: &e};

			// Pre conditions
			assert_eq!(buf.len(), 0, "buf should start empty");
			assert!(buf.get(0).is_none(), "should contain no event");

			println!("\nAPPEND\n");
			// Modifying
			buf.append_event(&event).expect("buf should have enough");

			println!("\nREAD\n");
			// Post conditions
			let actual = read(&buf, 0).expect("one event to be at 0");
			assert_eq!(actual.val, &e);
		}
	}
}
