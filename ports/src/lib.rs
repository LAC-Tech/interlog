//! This module exists purely for dependency purposes.
//! Test utils needs to know about this to make simulated storage

/// Where the events are persisted.
/// Written right now so I can simulate faulty storage.
/// Possible concrete implementations:
/// - Disk
/// - In-memory
/// - In-browser (WASM that calls to indexedDB?)
pub trait Storage {
    type Err;

    fn append(&mut self, data: &[u8]) -> Result<(), Self::Err>;
    fn read(&self) -> &[u8];
    fn size(&self) -> usize;
}
