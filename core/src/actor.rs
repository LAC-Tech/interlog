use crate::event;
use crate::fixed_capacity::Vec;
use crate::index::Index;
use crate::mem;
use crate::pervasives::*;

pub struct Actor {
	addr: Addr,
	log: Log,
	index: Index,
}

impl Actor {
	pub fn recv(&mut self, msg: Msg) {
		match msg {
			Msg::SyncRes(es) => {
				for e in es {
					self.log.txn.push(&e);
					self.index.insert(e.id);
				} /*
				 let ids = events |> List.map (fun e -> e.Event.id) in
				 Index.add_ids actor.index ids;
				 Dynarray.append_list actor.log events
				 */
			}
		}
	}
}

// The 'body' of each message will just be a pointer/
// It must come from some buffer somewhere (ie, TCP buffer)
enum Msg<'a> {
	SyncRes(event::Slice<'a>),
}

struct Log {
	txn: event::Buf,
	actual: event::Buf,
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
