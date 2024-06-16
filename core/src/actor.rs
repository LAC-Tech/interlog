use crate::event;
use crate::fixed_capacity::Vec;
use crate::index::Index;
use crate::pervasives::*;

pub struct Actor {
	addr: Addr,
	//log: Vec<event::Event>,
	index: Index,
}
