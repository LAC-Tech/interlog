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
/// Possible concrete implementations:
/// - Disk
/// - In-memory
/// - In-browser (WASM that calls to indexedDB?)
pub trait AppendOnly {
	/// Different concrete implementations will have different errors
	type WriteErr: core::fmt::Debug;
	fn used(&self) -> Qty;
	fn write(&mut self, data: &[u8]) -> Result<(), Self::WriteErr>;
	fn read(&self, buf: &mut [u8], offset: usize);
}
