//! Structs for reading and writing events from contiguous bytes.
use crate::fixed_capacity;
use crate::fixed_capacity::Vec;
use crate::mem;
use crate::pervasives::*;

/// This ID is globally unique.
/// TODO: is it worth trying to fit this into, say 128 bits? 80 bit replica ID,
/// 48 bit logical position.
#[repr(C)]
#[derive(
	bytemuck::Pod,
	bytemuck::Zeroable,
	Clone,
	Copy,
	Debug,
	Eq,
	PartialEq,
	PartialOrd,
	Ord,
)]
pub struct ID {
	/// Replica the event was first recorded at.
	pub origin: Addr,
	/// This can be thought of as a lamport clock, or the sequence number of
	/// the log.
	pub log_pos: LogPos,
}

impl ID {
	pub fn new<LP: Into<LogPos>>(origin: Addr, log_pos: LP) -> Self {
		Self { origin, log_pos: log_pos.into() }
	}
}

/// This is written before every event in the log, and allows reading out
/// payloads which are of variable length.
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
#[repr(C)]
struct Header {
	id: ID,
	byte_len: usize,
}

pub const HEADER_SIZE: usize = core::mem::size_of::<Header>();

/// An immutable record of some event. The core data structure of interlog.
/// The term "event" comes from event sourcing, but this couldd also be thought
/// of as a record or entry.
#[derive(Clone, Debug)]
pub struct Event<'a> {
	pub id: ID,
	pub payload: &'a [mem::Word],
}

impl<'a> Event<'a> {
	/// Number of bytes the event will take up, including the header
	pub fn on_disk_size(&self) -> usize {
		let raw_size = HEADER_SIZE + self.payload.len();
		// align to 8
		(raw_size + 7) & !7
	}
}

pub fn read(bytes: &[mem::Word], byte_offset: usize) -> Option<Event<'_>> {
	let header_region = mem::Region::new(byte_offset, HEADER_SIZE);
	let header_bytes = header_region.read(bytes)?;
	let &Header { id, byte_len } = bytemuck::from_bytes(header_bytes);
	let payload_region = header_region.next(byte_len);
	let payload = payload_region.read(bytes)?;
	Some(Event { id, payload })
}

pub fn append(buf: &mut Vec<mem::Word>, event: &Event) -> fixed_capacity::Res {
	let byte_len = event.payload.len();
	let header_region = mem::Region::new(buf.len(), HEADER_SIZE);
	let header = Header { byte_len, id: event.id };
	let payload_region = header_region.next(byte_len);
	let next_offset = buf.len() + event.on_disk_size();
	buf.resize(next_offset)?;
	let header_bytes = bytemuck::bytes_of(&header);

	header_region.write(buf, header_bytes).expect("fixvec to be resized");
	payload_region.write(buf, event.payload).expect("fixvec to be resized");

	Ok(())
}

/*
#[derive(bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Batch<const N_INDICES: usize, const N_BYTES: usize> {
	indices: [DiskOffset; N_INDICES],
	bytes: [u8; N_BYTES],
}
*/

pub struct View<'a> {
	bytes: &'a [mem::Word],
	byte_index: usize,
}

impl<'a> View<'a> {
	pub fn new(bytes: &'a [mem::Word]) -> Self {
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
			e in prop::collection::vec(any::<mem::Word>(), 0..=8)
		) {
			// Setup
			let mut rng = rand::thread_rng();
			let mut buf = Vec::new(256);
			let origin = Addr::new(rng.gen());
			let id = ID::new(origin, LogPos(0));
			let event = Event {id, payload: &e};

			// Pre conditions
			assert_eq!(buf.len(), 0, "buf should start empty");
			assert!(read(&buf, 0).is_none(), "should contain no event");

			// Modifying
			append(&mut buf, &event).expect("buf should have enough");

			// Post conditions
			let actual = read(&buf, 0).expect("one event to be at 0");
			assert_eq!(actual.payload, &e);
		}
	}
}
