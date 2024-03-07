//! Structs for reading and writing events from contiguous bytes.
use crate::mem;
use crate::replica_id::ReplicaID;
use crate::unit;
use crate::util::{FixVec, FixVecRes};

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
	byte_len: unit::Byte,
	id: ID
}

impl Header {
	const SIZE: unit::Byte = unit::Byte(std::mem::size_of::<Self>());
}

pub struct WriteInfo {
	header_region: mem::Region,
	header: Header,
	payload_segment: mem::Region,
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
		(Header::SIZE + self.payload_len()).align()
	}

	#[inline]
	fn payload_len(&self) -> unit::Byte {
		self.payload.len().into()
	}

	fn write_info(&self, offset: unit::Byte) -> WriteInfo {
		let header_region = mem::Region::new(offset, Header::SIZE);

		WriteInfo {
			header: Header { byte_len: self.payload_len(), id: self.id },
			header_region,
			payload_segment: header_region.next(self.payload_len()),
			next_offset: offset + self.on_disk_size()
		}
	}
}

pub fn read<'a, B, O>(bytes: B, offset: O) -> Option<Event<'a>>
where
	B: AsRef<[u8]>,
	O: Into<unit::Byte>
{
	let header_region = mem::Region::new(offset.into(), Header::SIZE);
	let header_bytes = mem::read(&bytes, header_region)?;
	let &Header { id, byte_len } = bytemuck::from_bytes(header_bytes);
	let payload_region = header_region.next(byte_len);
	let payload = mem::read(&bytes, payload_region)?;
	Some(Event { id, payload })
}

pub fn append(buf: &mut FixVec<u8>, event: &Event) -> FixVecRes {
	let byte_len = event.payload_len();
	let offset = buf.len().into();
	let header_region = mem::Region::new(offset, Header::SIZE);
	let header = Header { byte_len, id: event.id };
	let payload_segment = header_region.next(byte_len);
	let next_offset = offset + event.on_disk_size();
	buf.resize(next_offset.into(), 0)?;
	let header_bytes = bytemuck::bytes_of(&header);

	mem::write(buf, &header_region, header_bytes);
	mem::write(buf, &payload_segment, event.payload);

	Ok(())
}

pub struct View<'a> {
	bytes: &'a [u8],
	index: unit::Byte
}

impl<'a> View<'a> {
	pub fn new<R: AsRef<[u8]>>(mem: &'a R) -> Self {
		Self { bytes: mem.as_ref(), index: 0.into() }
	}
}

impl<'a> Iterator for View<'a> {
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
	//use crate::mem::Readable;
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
			let event = Event {id: ID::new(replica_id, 0), payload: &e};

			// Pre conditions
			assert_eq!(mem::size(buf), 0.into(), "buf should start empty");
			assert!(read(&buf, 0).is_none(), "should contain no event");

			println!("\nAPPEND\n");
			// Modifying
			append(&mut buf, &event).expect("buf should have enough");

			println!("\nREAD\n");
			// Post conditions
			let actual = read(&buf, 0).expect("one event to be at 0");
			assert_eq!(actual.payload, &e);
		}
	}
}
