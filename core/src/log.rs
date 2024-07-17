use core::ops::RangeBounds;

use crate::event;
use crate::fixcap;
use crate::fixcap::Vec;
use crate::mem;
use crate::pervasives::*;
use crate::storage;

pub struct Log<AOS: storage::AppendOnly> {
	pub addr: Addr,
	enqueued: Enqueued,
	committed: Committed<AOS>,
}

impl<AOS: storage::AppendOnly> Log<AOS> {
	pub fn new(addr: Addr, config: Config, aos: AOS) -> Self {
		let enqueued = Enqueued::new(config.max_txn_events, config.txn_size);
		let committed = Committed::new(config.max_events, aos);
		Self { addr, enqueued, committed }
	}

	pub fn recv(&mut self, msg: Msg, send: impl Fn(Msg, Addr)) {
		match msg.inner {
			InnerMsg::SyncRes(events) => {
				let write_res = events
					.into_iter()
					.try_for_each(|e| {
						self.enqueued.append(&e).map_err(ReplicaErr::Enqueue)
					})
					.and_then(|()| self.commit().map_err(ReplicaErr::Commit));
				if let Err(err) = write_res {
					self.rollback();
					let outgoing_msg =
						Msg { inner: InnerMsg::Err(err), origin: self.addr };
					send(outgoing_msg, msg.origin);
				}
			}
			InnerMsg::Err(err) => {
				panic!("TODO: implement error logging. {:?}", err)
			}
		}
	}

	pub fn enqueue(&mut self, payload: &[u8]) -> Result<(), EnqueueErr> {
		let origin = self.addr;
		let pos = self.committed.count() + self.enqueued.count();
		let id = event::ID { origin, pos };
		let e = event::Event { id, payload };

		self.enqueued.append(&e)
	}

	/// Returns number of events committed
	pub fn commit(&mut self) -> Result<usize, CommitErr> {
		let offsets = &self.enqueued.offsets;
		let result = self
			.committed
			.append(offsets, self.enqueued.events.as_bytes())
			.map(|()| offsets.len());

		self.enqueued.reset(self.committed.last_offset());
		result
	}

	pub fn rollback(&mut self) {
		self.enqueued.reset(self.committed.last_offset());
	}

	// TODO: indexable actor?
	pub fn read(
		&self,
		range: impl RangeBounds<usize>,
		buf: &mut event::Buf,
	) -> fixcap::Res {
		self.committed.read(range, buf)
	}
}

struct Enqueued {
	offsets: Vec<storage::Qty>,
	next_committed_offset: storage::Qty,
	events: event::Buf,
}

impl Enqueued {
	fn new(max_txn_events: LogicalQty, txn_size: storage::Qty) -> Self {
		Self {
			offsets: Vec::new(max_txn_events.0),
			next_committed_offset: storage::Qty(0),
			events: event::Buf::new(txn_size),
		}
	}

	fn append(&mut self, e: &event::Event) -> Result<(), EnqueueErr> {
		let last_enqueued_offset =
			self.offsets.last().copied().unwrap_or(self.next_committed_offset);

		let offset = last_enqueued_offset + e.size();

		self.offsets.push(offset).map_err(EnqueueErr::Offsets)?;
		self.events.push(e).map_err(EnqueueErr::Events)
	}

	fn reset(&mut self, next_committed_offset: storage::Qty) {
		self.offsets.clear();
		self.events.clear();
		self.next_committed_offset = next_committed_offset;
	}

	fn count(&self) -> LogicalQty {
		LogicalQty(self.offsets.len())
	}
}

struct Committed<AOS: storage::AppendOnly> {
	/// This is always one greater than the number of events stored; the last
	/// element is the next offset of the next event appended
	offsets: Vec<storage::Qty>,
	events: AOS,
}

impl<AOS: storage::AppendOnly> Committed<AOS> {
	fn new(max_events: LogicalQty, aos: AOS) -> Self {
		let mut offsets = Vec::new(max_events.0);
		offsets
			.push(storage::Qty(0))
			.expect("max events should be more than 0");

		Self { offsets, events: aos }
	}

	fn append(
		&mut self,
		offsets: &[storage::Qty],
		events: &[mem::Word],
	) -> Result<(), CommitErr> {
		self.offsets.extend_from_slice(offsets).map_err(CommitErr::Offsets)?;
		self.events.append(events).map_err(CommitErr::Events)
	}

	fn last_offset(&self) -> storage::Qty {
		self.offsets
			.last()
			.copied()
			.expect("offsets should always contain at least one element")
	}

	fn count(&self) -> LogicalQty {
		LogicalQty(self.offsets.len())
	}

	fn read<R: RangeBounds<usize>>(
		&self,
		range: R,
		buf: &mut event::Buf,
	) -> fixcap::Res {
		buf.clear();
		let range = (
			range.start_bound().cloned(),
			range.end_bound().cloned().map(|n| n + 1),
		);

		let offsets: &[storage::Qty] = &self.offsets[range];

		offsets
			.first()
			.cloned()
			.zip(offsets.last().cloned())
			.map(|(start, end)| (start.0, end.0 - start.0))
			.map_or(Ok(()), |(offset, len)| {
				let buf = buf.as_mut_vec();
				buf.fill(len, |words| self.events.read(words, offset))
			})
	}
}

pub struct Config {
	pub txn_size: storage::Qty,
	pub max_txn_events: LogicalQty,
	pub max_events: LogicalQty,
}

#[derive(Debug)]
pub enum EnqueueErr {
	Offsets(mem::Overrun),
	Events(mem::Overrun),
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub enum CommitErr {
	Offsets(mem::Overrun),
	Events(storage::Overrun),
}

#[derive(Debug)]
pub enum ReplicaErr {
	Enqueue(EnqueueErr),
	Commit(CommitErr),
}

pub struct Msg<'a> {
	inner: InnerMsg<'a>,
	origin: Addr,
}

// The 'body' of each message will just be a pointer/
// It must come from some buffer somewhere (ie, TCP buffer)
pub enum InnerMsg<'a> {
	/*
	 * TODO: make illegal states un-representable
	 * This should be consecutive events, ordered by position, grouped by address
	 */
	SyncRes(event::Slice<'a>),
	// String to avoid parameterising every message by AOS::WriteErr
	Err(ReplicaErr),
}

#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;
	use rand::prelude::*;

	struct AppendOnlyMemory(fixcap::Vec<u8>);

	impl AppendOnlyMemory {
		fn new(capacity: usize) -> Self {
			Self(fixcap::Vec::new(capacity))
		}
	}

	impl storage::AppendOnly for AppendOnlyMemory {
		fn used(&self) -> storage::Qty {
			storage::Qty(self.0.len())
		}

		fn append(&mut self, data: &[u8]) -> Result<(), storage::Overrun> {
			self.0
				.extend_from_slice(data)
				// TODO: should storage overrun have the same fields?
				.map_err(|mem::Overrun { .. }| storage::Overrun)
		}

		fn read(&self, buf: &mut [u8], offset: usize) {
			buf.copy_from_slice(&self.0[offset..offset + buf.len()])
		}
	}

	#[test]
	fn enqueue_commit_and_read() {
		let mut rng = SmallRng::from_entropy();
		let mut log = Log::new(
			Addr::new(&mut rng),
			Config {
				max_events: LogicalQty(3),
				txn_size: storage::Qty(4096),
				max_txn_events: LogicalQty(3),
			},
			AppendOnlyMemory::new(4096),
		);

		let mut read_buf = event::Buf::new(storage::Qty(128));

		log.enqueue(b"I have known the arcane law").unwrap();
		assert_eq!(log.commit(), Ok(1));
		log.read(0..=0, &mut read_buf).unwrap();
		let actual = &read_buf.into_iter().last().unwrap();
		assert_eq!(actual.payload, b"I have known the arcane law");

		log.enqueue(b"On strange roads, such visions met").unwrap();
		assert_eq!(log.commit(), Ok(1));
		log.read(1..=1, &mut read_buf).unwrap();
		let actual = &read_buf.into_iter().last().unwrap();
		assert_eq!(
			core::str::from_utf8(actual.payload).unwrap(),
			"On strange roads, such visions met"
		);
	}
}
/*
/// A fixed sized structure that caches the latest entries in the log
/// (LIFO caching). The assumption is that things recently added are most
/// likely to be read out again.
///
/// To do this I'm using a single circular buffer, with two "write pointers".
/// At any given point in time the buffer will have two contiguous segments,
/// populated with events.
///
/// The reason to keep the two segments contiguous is so they can be easily
/// memcpy'd. So no event is split by the circular buffer.
///
/// There is a Top Segment, and a Bottom Segment.
///
/// We start with a top segment. The bottom segement gets created when we
/// reach the end of the circular buffer and need to wrap around, eating into
/// the former top segment.
///
/// Example:
/// ```text
/// ┌---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┐
/// | A | A | B | B | B |   | X | X | X | Y | Z | Z | Z | Z |   |   |
/// └---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┘
/// ```
///
/// The top segment contains events A and B, while the bottom segment contains
/// X, Y and Z
/// As more events are added, they will be appended after B, overwriting the
/// bottom segment, til it wraps round again.
struct ReadCache {
	mem: Box<[mem::Word]>,
	/// Everything above this is in this cache
	logical_start: usize,
	a: Region,
	b: Region, // pos is always 0 but it's just easier
}

impl fmt::Debug for ReadCache {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ReadCache")
			.field("logical_start", &self.logical_start)
			.field("a", &self.read_a())
			.field("b", &self.read_b())
			.finish()
	}
}

impl ReadCache {
	fn new(capacity: usize) -> Self {
		let mem = vec![0; capacity].into_boxed_slice();
		let logical_start = 0;
		let a = Region::ZERO;
		let b = Region::ZERO; // by definition B always starts at 0
		Self { mem, logical_start, a, b }
	}

	fn extend(
		region: &mut Region,
		dest: &mut [mem::Word],
		src: &[mem::Word],
	) -> Result<(), mem::WriteErr> {
		let extension = Region::new(region.len, src.len());

		extension.write(dest, src)?;
		region.lengthen(src.len());
		Ok(())
	}

	fn overlapping_regions(&self) -> bool {
		self.b.end() > self.a.pos
	}

	fn update(&mut self, es: &[mem::Word]) -> WriteRes {
		let result = match (self.a.empty(), self.b.empty()) {
			(true, true) => {
				self.set_logical_start(es);
				Self::extend(&mut self.a, &mut self.mem, es)
			}
			(false, true) => match Self::extend(&mut self.a, &mut self.mem, es)
			{
				Ok(()) => Ok(()),
				Err(mem::WriteErr) => self.wrap_around(es),
			},
			(_, false) => self.wrap_around(es),
		};

		// Post conditions
		assert_eq!(self.b.pos, 0);
		assert!(!self.overlapping_regions());

		result.map_err(CommitErr::ReadCache)
	}

	fn read(&self, relative_byte_pos: usize) -> Option<event::Event<'_>> {
		let a_bytes = self.read_a();
		let e: Option<_> = event::read(a_bytes, relative_byte_pos);
		if let Some(_) = e {
			return e;
		}

		let relative_byte_pos = relative_byte_pos - a_bytes.len();
		return event::read(self.read_b(), relative_byte_pos);
	}

	fn set_logical_start(&mut self, es: &[mem::Word]) {
		let first_event = event::read(es, 0).expect("no event found at 0");
		self.logical_start = first_event.id.log_pos.0;
	}

	fn wrap_around(&mut self, es: &[mem::Word]) -> Result<(), mem::WriteErr> {
		match self.new_a_pos(es) {
			// Truncate A and write to B
			Some(new_a_pos) => {
				self.a.change_pos(new_a_pos);
				Self::extend(&mut self.b, self.mem.as_mut(), es)
			}
			// We've searched past the end of A and found nothing.
			// B is now A
			None => {
				self.a = Region::new(0, self.b.end());
				self.b = Region::ZERO;
				match Self::extend(&mut self.a, self.mem.as_mut(), es) {
					Ok(()) => Ok(()),
					// the new A cannot fit, erase it.
					// would not occur with buf size 2x or more max event size
					Err(mem::WriteErr) => {
						self.a = Region::ZERO;
						self.set_logical_start(es);
						Self::extend(&mut self.a, self.mem.as_mut(), es)
					}
				}
			}
		}
	}

	fn new_a_pos(&self, es: &[mem::Word]) -> Option<usize> {
		let new_b_end = self.b.end() + es.len();
		event::View::new(self.read_a())
			.scan(0, |offset, e| {
				*offset += e.on_disk_size();
				Some(*offset)
			})
			.find(|&offset| offset > new_b_end)
	}

	fn read_a(&self) -> &[mem::Word] {
		self.a.read(&self.mem).expect("a range to be correct")
	}

	fn read_b(&self) -> &[mem::Word] {
		self.b.read(&self.mem).expect("b range to be correct")
	}
}
*/
