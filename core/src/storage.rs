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

#[derive(Debug)]
pub struct Overrun;

/// Where the events are persisted.
/// Written right now so I can simulate faulty storage.
/// Possible concrete implementations:
/// - Disk
/// - In-memory
/// - In-browser (WASM that calls to indexedDB?)
pub trait AppendOnly {
	fn used(&self) -> Qty;
	fn append(&mut self, data: &[u8]) -> Result<(), Overrun>;
	fn read(&self, buf: &mut [u8], offset: usize);
}
