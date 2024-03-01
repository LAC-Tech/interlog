//! Structs for reading and writing events from contiguous bytes.
use crate::replica_id::ReplicaID;
use crate::unit;
use crate::util::{FixVec, FixVecRes, Segment, Segmentable};

/// This ID is globally unique.
/// TODO: is it worth trying to fit this into, say 128 bits? 80 bit replica ID,
/// 48 bit logical position.
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
pub struct ID {
	/// Replica the event was first recorded at.
	pub origin: ReplicaID,
	/// This can be thought of as a lamport clock, or the sequence number of
	/// the log.
	pub pos: unit::Logical
}

impl ID {
	fn new<P: Into<unit::Logical>>(origin: ReplicaID, disk_pos: P) -> Self {
		ID { origin, pos: disk_pos.into() }
	}
}

/// This is written before every event in the log, and allows reading out
/// payloads which are of variable length.
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
#[repr(C)]
struct Header {
	byte_len: usize,
	id: ID
}

impl Header {
	const SIZE: usize = std::mem::size_of::<Self>();

	fn range(start: unit::Byte) -> Segment {
		Segment::new(start.0, Self::SIZE)
	}
}

/// An immutable record of some event. The core data structure of interlog.
/// The term "event" comes from event sourcing, but this couldd also be thought
/// of as a record or entry.
/// TODO: Enforce that Payload is 2MiB or less?
#[derive(Clone, Debug)]
pub struct Event<'a> {
	pub id: ID,
	pub payload: &'a [u8]
}

impl<'a> Event<'a> {
	/// Number of bytes the event will take up, including the header
	pub fn on_disk_size(&self) -> unit::Byte {
		let size: unit::Byte = (Header::SIZE + self.payload.len()).into();
		size.align()
	}
}

pub struct Buf(FixVec<u8>);

impl Buf {
	pub fn new(capacity: unit::Byte) -> Self {
		Self(FixVec::new(capacity.into()))
	}

	pub fn append(&mut self, event: &Event) -> FixVecRes {
		let Event { id, payload: val } = *event;
		let offset = self.0.len().into();
		let header_segment = Header::range(offset);
		let byte_len = val.len();
		let val_segment = header_segment.next(byte_len);
		let new_len = unit::Byte(val_segment.end).align();
		self.0.resize(new_len.into(), 0)?;

		let header = Header { byte_len, id };
		let header = bytemuck::bytes_of(&header);

		self.0[header_segment.range()].copy_from_slice(header);
		self.0[val_segment.range()].copy_from_slice(val);

		Ok(())
	}

	pub fn extend(&mut self, other: &Buf) -> FixVecRes {
		self.0.extend_from_slice(other.as_bytes())
	}

	pub fn read<O>(&self, offset: O) -> Option<Event<'_>>
	where
		O: Into<unit::Byte>
	{
		let header_segment = Header::range(offset.into());
		let header_bytes = self.0.segment(&header_segment)?;
		let &Header { id, byte_len } = bytemuck::from_bytes(header_bytes);
		let val_segment = header_segment.next(byte_len);
		let val = self.0.segment(&val_segment)?;
		Some(Event { id, payload: val })
	}

	pub fn clear(&mut self) {
		self.0.clear()
	}

	pub fn as_bytes(&self) -> &[u8] {
		&self.0
	}

	pub fn byte_len(&self) -> unit::Byte {
		self.0.len().into()
	}

	pub fn byte_capacity(&self) -> unit::Byte {
		self.0.capacity().into()
	}
}

pub struct BufIntoIterator<'a> {
	event_buf: &'a Buf,
	index: unit::Byte
}

impl<'a> Iterator for BufIntoIterator<'a> {
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		let result = self.event_buf.read(self.index)?;
		self.index += result.on_disk_size();
		Some(result)
	}
}

impl<'a> IntoIterator for &'a Buf {
	type Item = Event<'a>;
	type IntoIter = BufIntoIterator<'a>;

	fn into_iter(self) -> Self::IntoIter {
		BufIntoIterator { event_buf: self, index: 0.into() }
	}
}

/*
pub fn read<B, O>(bytes: &B, offset: O) -> Option<Event<'_>>
where
	B: Segmentable<u8>,
	O: Into<unit::Byte>
{
	let header_segment = Header::range(offset.into());
	let header_bytes = bytes.segment(&header_segment)?;
	let &Header { id, byte_len } = bytemuck::from_bytes(header_bytes);
	let val_segment = header_segment.next(byte_len);
	let val = bytes.segment(&val_segment)?;
	Some(Event { id, payload: val })
}
*/

#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;

	proptest! {
		#[test]
		fn rw_single_event(
			e in prop::collection::vec(any::<u8>(), 0..=8)
		) {
			// Setup
			let mut rng = rand::thread_rng();
			let mut buf = Buf::new(256.into());
			let replica_id = ReplicaID::new(&mut rng);
			let event = Event {id: ID::new(replica_id, 0), payload: &e};

			// Pre conditions
			assert_eq!(buf.byte_len(), 0.into(), "buf should start empty");
			assert!(&buf.read(0).is_none(), "should contain no event");

			println!("\nAPPEND\n");
			// Modifying
			buf.append(&event).expect("buf should have enough");

			println!("\nREAD\n");
			// Post conditions
			let actual = &buf.read(0).expect("one event to be at 0");
			assert_eq!(actual.payload, &e);
		}
	}
}
