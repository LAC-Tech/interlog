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

pub struct WriteInfo {
	header_segment: Segment,
	header: Header,
	payload_segment: Segment,
	next_offset: unit::Byte
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

	fn write_info(&self, offset: unit::Byte) -> WriteInfo {
		let Event { id, payload } = *self;
		let byte_len = payload.len();
		let header_segment = Header::range(offset);

		WriteInfo {
			header: Header { byte_len, id },
			header_segment,
			payload_segment: header_segment.next(payload.len()),
			next_offset: offset + self.on_disk_size()
		}
	}
}

pub fn read<'a, S, O>(bytes: S, offset: O) -> Option<Event<'a>>
where
	S: Segmentable<u8>,
	O: Into<unit::Byte>
{
	let header_segment = Header::range(offset.into());
	let header_bytes = bytes.segment(&header_segment)?;
	let &Header { id, byte_len } = bytemuck::from_bytes(header_bytes);
	let payload_segment = header_segment.next(byte_len);
	let payload = bytes.segment(&payload_segment)?;
	Some(Event { id, payload })
}

pub struct Buf(FixVec<u8>);

impl Buf {
	pub fn new(capacity: unit::Byte) -> Self {
		Self(FixVec::new(capacity.into()))
	}

	pub fn append(&mut self, event: &Event) -> FixVecRes {
		let offset = self.0.len().into();

		let WriteInfo { header_segment, header, payload_segment, next_offset } =
			event.write_info(offset);

		self.0.resize(next_offset.into(), 0)?;
		let header_bytes = bytemuck::bytes_of(&header);
		self.0[header_segment.range()].copy_from_slice(header_bytes);
		self.0[payload_segment.range()].copy_from_slice(event.payload);

		Ok(())
	}

	pub fn extend(&mut self, other: &Buf) -> FixVecRes {
		self.0.extend_from_slice(other.as_bytes())
	}

	pub fn read<O>(&self, offset: O) -> Option<Event<'_>>
	where
		O: Into<unit::Byte>
	{
		read(self.0, offset)
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

	pub fn into_iter(&self) -> EventIntoIterator<'_> {
		EventIntoIterator { bytes: &self.0, index: unit::Byte(0) }
	}
}

pub struct EventIntoIterator<'a> {
	bytes: &'a [u8],
	index: unit::Byte
}

impl<'a> EventIntoIterator<'a> {
	pub fn new(bytes: &'a [u8]) -> Self {
		Self { bytes, index: 0.into() }
	}
}

impl<'a> Iterator for EventIntoIterator<'a> {
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		let result = read(self.bytes, self.index)?;
		self.index += result.on_disk_size();
		Some(result)
	}
}

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
