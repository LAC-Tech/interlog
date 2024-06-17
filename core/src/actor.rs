use crate::event;
use crate::fixed_capacity;
use crate::index;
use crate::index::Index;
use crate::mem;
use crate::pervasives::*;

pub struct Actor<N: Network> {
	addr: N::Addr,
	log: log::Log,
	index: Index,
}

impl<N: Network> Actor<N> {
	pub fn recv(&mut self, src: N::Addr, msg: Msg, network: &mut N) {
		match msg {
			Msg::SyncRes(es) => {
				let txn_result: Result<(), AppendErr> = es
					.into_iter()
					.map(|e| {
						let new_pos = self
							.log
							.append(&e)
							.map_err(AppendErr::LogAppend)?;
						self.index
							.insert(e.id, new_pos)
							.map_err(AppendErr::IndexInsert)?;
						Ok(())
					})
					.collect::<Result<(), AppendErr>>()
					.and_then(|()| {
						self.index.commit().map_err(AppendErr::IndexCommit)?;
						self.log.commit().map_err(AppendErr::LogCommit)?;
						Ok(())
					});

				if let Err(err) = txn_result {
					self.index.rollback();
					self.log.rollback();
					network.send(Msg::AppendErr(err), src);
					// TODO: send err msg across the network
				}
			}
			Msg::AppendErr(_) => {
				panic!("TODO: implement error logging")
			}
		}
	}
}

trait Network {
	type Addr;

	// Fire and forget message passing semantics
	fn send(&mut self, msg: Msg, dest: Self::Addr);
}

enum AppendErr {
	LogAppend(fixed_capacity::Overrun),
	IndexInsert(index::InsertErr),
	LogCommit(log::CommitErr),
	IndexCommit(index::CommitErr),
}

// The 'body' of each message will just be a pointer/
// It must come from some buffer somewhere (ie, TCP buffer)
enum Msg<'a> {
	SyncRes(event::Slice<'a>),
	AppendErr(AppendErr),
}

mod log {
	use super::*;

	// Can't think of any yet but I am sure there are LOADS
	pub struct CommitErr;

	pub struct Log {
		txn: event::Buf,
		actual: event::Buf,
	}

	impl Log {
		pub fn append(
			&mut self,
			e: &event::Event,
		) -> Result<StoragePos, fixed_capacity::Overrun> {
			let result = self.actual.len() + self.txn.len();
			self.txn.push(e)?;
			Ok(result)
		}

		pub fn commit(&mut self) -> Result<(), CommitErr> {
			panic!("TODO")
		}

		pub fn rollback(&mut self) {
			self.txn.clear();
		}
	}
}

/*
mod log {
	use super::*;

	enum AppendErr {
		NoCapacity,
	}
	enum CommitErr {}

	pub struct Log<AOS: AppendOnlyStorage> {
		aos: AOS,
		txn_buffer: Vec<mem::Word>,
	}
	impl<AOS: AppendOnlyStorage> Log<AOS> {
		pub fn append(&mut self, e: event::ID) -> Result<(), AppendErr> {
			panic!("TODO");
		}
		pub fn commit(&mut self) -> Result<(), CommitErr> {
			panic!("TODO")
		}
		pub fn rollback(&mut self) {
			self.txn_buffer.clear();
		},w
	}
}


enum AppendErr {

}

pub trait AppendOnlyStorage {
	pub fn append(
		&mut self,
		words: &[mem::Word],
	) -> Result<(), Self::AppendErr>;
}

struct AppendOnlyMemory<const MAX_WORDS: usize> {
	mem: alloc::boxed::Box<[mem::Word; MAX_WORDS]>,
}

impl<const MAX_WORDS: usize> AppendOnlyStorage for AppendOnlyMemory<MAX_WORDS> {}
*/
