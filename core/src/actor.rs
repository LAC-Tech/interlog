use crate::event;
use crate::fixed_capacity::Vec;
use crate::index::Index;
use crate::mem;
use crate::pervasives::*;

pub struct Actor<L: Log> {
	addr: Addr,
	storage: Storage<L>,
	index: Index,
}

impl<L: Log> Actor<L> {
	pub fn recv(msg: Msg) {
		match msg {
			Msg::SyncRes(es) => {
				for e in es {}

				/*
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

struct Storage<L: Log> {
	log: L,
	txn_buffer: Vec<mem::Word>,
}

impl<L: Log> Storage<L> {
	fn enqueue(&mut self, e: event::ID) {}
	fn commit(&mut self) {}
	fn rollback(&mut self) {}
}

pub trait Log {
	//pub fn push(&mut self, )
}

struct InMemLog<const MAX_WORDS: usize> {
	mem: alloc::boxed::Box<[mem::Word; MAX_WORDS]>,
}

impl<const MAX_WORDS: usize> Log for InMemLog<MAX_WORDS> {}
