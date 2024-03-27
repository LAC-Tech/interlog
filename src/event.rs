//! Structs for reading and writing events from contiguous bytes.
use crate::fixvec;
use crate::fixvec::FixVec;
use crate::log_id::LogID;
use crate::util::region::Region;

/// This ID is globally unique.
/// TODO: is it worth trying to fit this into, say 128 bits? 80 bit replica ID,
/// 48 bit logical position.
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
pub struct ID {
	/// Replica the event was first recorded at.
	pub origin: LogID,
	/// This can be thought of as a lamport clock, or the sequence number of
	/// the log.
	pub logical_pos: usize
}

impl ID {
	fn new(origin: LogID, logical_pos: usize) -> Self {
		ID { origin, logical_pos }
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
	pub fn on_disk_size(&self) -> usize {
		let raw_size = Header::SIZE + self.payload.len();
		// align to 8
		(raw_size + 7) & !7
	}
}

pub fn read(bytes: &[u8], byte_offset: usize) -> Option<Event<'_>> {
	let header_region = Region::new(byte_offset, Header::SIZE);
	let header_bytes = header_region.read(bytes)?;
	let &Header { id, byte_len } = bytemuck::from_bytes(header_bytes);
	let payload_region = header_region.next(byte_len);
	let payload = payload_region.read(bytes)?;
	Some(Event { id, payload })
}

pub fn append(buf: &mut FixVec<u8>, event: &Event) -> fixvec::Res {
	let byte_len = event.payload.len();
	let header_region = Region::new(buf.len(), Header::SIZE);
	let header = Header { byte_len, id: event.id };
	let payload_region = header_region.next(byte_len);
	let next_offset = buf.len() + event.on_disk_size();
	buf.resize(next_offset, 0)?;
	let header_bytes = bytemuck::bytes_of(&header);

	header_region.write(buf, header_bytes).expect("fixvec to be resized");
	payload_region.write(buf, event.payload).expect("fixvec to be resized");

	Ok(())
}

pub struct View<'a> {
	bytes: &'a [u8],
	byte_index: usize
}

impl<'a> View<'a> {
	pub fn new(bytes: &'a [u8]) -> Self {
		Self { bytes, byte_index: 0 }
	}
}

impl<'a> Iterator for View<'a> {
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		let result = read(self.bytes, self.byte_index)?;
		self.byte_index += result.on_disk_size();
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
			let mut buf = FixVec::new(256);
			let replica_id = LogID::new(&mut rng);
			let event = Event {id: ID::new(replica_id, 0), payload: &e};

			// Pre conditions
			assert_eq!(buf.len(), 0, "buf should start empty");
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
