use crate::event;
use crate::fixed_capacity::Vec;
use crate::index::Index;
use crate::mem;
use crate::pervasives::*;

struct Actor<L: Log> {
	addr: Addr,
	storage: Storage<L>,
	index: Index,
}

impl<L: Log> Actor<L> {
	fn recv(msg: Msg) {
		panic!("TODO")
	}
}

enum Msg {
	SyncResponse(),
}

struct Storage<L: Log> {
	log: L,
	txn_buffer: Vec<mem::Word>,
}

trait Log {}

struct InMemLog<const MAX_EVENTS: usize, const MAX_WORDS: usize> {
	indices: alloc::boxed::Box<[DiskOffset; MAX_EVENTS]>,
	mem: alloc::boxed::Box<[mem::Word; MAX_EVENTS]>,
}

impl<const MAX_EVENTS: usize, const MAX_WORDS: usize> Log
	for InMemLog<MAX_EVENTS, MAX_WORDS>
{
}
