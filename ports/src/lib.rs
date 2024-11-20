//! This module exists purely for dependency purposes.
//! Test utils needs to know about this to make simulated storage

/// Where the events are persisted.
/// Written right now so I can simulate faulty storage.
/// Possible concrete implementations:
/// - Disk
/// - In-memory
/// - In-browser (WASM that calls to indexedDB?)
pub trait Storage {
	fn append(&mut self, data: &[u8]);
	fn as_slice(&self) -> &[u8];
	fn size(&self) -> usize;
}
