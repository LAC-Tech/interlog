use crate::fixed_capacity::Vec;

#[derive(Debug)]
pub enum WriteErr {
	Full,
}

/// Position of a word, in either memory or disk
#[derive(
	Clone,
	Copy,
	Debug,
	Default,
	PartialEq,
	PartialOrd,
	bytemuck::Pod,
	bytemuck::Zeroable,
	derive_more::Add,
	derive_more::Sum,
)]
#[repr(transparent)]
pub struct Qty(pub usize);

/// Where the events are persisted.
/// Written right now so I can simulate faulty storage.
/// (Only concrete implementation I can think of is an append only file)
pub trait AppendOnly {
	fn used(&self) -> Qty;
	fn write(&mut self, data: &[u8]) -> Result<(), WriteErr>;
	fn read(&self, buf: &mut [u8], offset: usize);
}
