#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use fs::OFlags;
use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};
use std::io::Read;

type O = OFlags;

#[derive(bytemuck::Zeroable, bytemuck::Pod, Clone, Copy, Debug)]
#[repr(C)]
struct EventID { origin: u128, pos: usize }
const EVENT_ID_LEN: usize = std::mem::size_of::<EventID>();

#[derive(Debug)]
struct Event<'a> { id: EventID, val: &'a [u8] }

struct EventIndex { pos: usize, len: usize }

struct EventBuffer { indices: Vec<EventIndex>, bytes: Vec<u8> }

impl EventBuffer {
    fn new() -> Self {
        Self { indices: vec![], bytes: vec![] }
    }

    fn write(&mut self, e: Event) {
        let index = EventIndex {
            pos: self.bytes.len(),
            len: EVENT_ID_LEN + e.val.len(),
        };

        self.bytes.extend_from_slice(bytemuck::bytes_of(&e.id));
        self.bytes.extend_from_slice(e.val);

        self.indices.push(index);
    }

    fn read(&self, i: usize) -> Event {
        let index = &self.indices[i];
        let id_range = index.pos..index.pos + EVENT_ID_LEN;
        let val_range = index.pos + EVENT_ID_LEN..index.len;

        Event {
            id: *bytemuck::from_bytes(&self.bytes[id_range]),
            val: &self.bytes[val_range],
        }
    }
}

struct ReplicaID(u128);

impl ReplicaID {
    fn new<R: Rng>(rng: &mut R) -> Self {
        Self(rng.gen())
    }
}

impl core::fmt::Display for ReplicaID {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{:x}", self.0)
    }
}

enum WriteErr {}

type WriteRes<L> = Result<L, WriteErr>;

trait Replica {
    // Events local to the replica, that don't yet have an ID
    fn local_write<const N: usize>(
        &mut self, events: [&[u8]; N]
    ) -> WriteRes<[ReplicaID; N]>;

    // events that have already been recorded on other replicas
    // designed to be used by the sync protocol
    fn remote_write(&mut self, event_slice: EventBuffer) -> WriteRes<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_and_write_events_onto_buffer() {
        let val: u64 = 42;
        let bytes = bytemuck::bytes_of(&val);
        let e = Event {
            id: EventID { origin: 0, pos: 0 },
            val: bytes,
        };

        let mut es = EventBuffer::new(); 

        es.write(e);
        let actual = es.read(0);

        assert_eq!(val, *bytemuck::from_bytes(actual.val))
    }
}

fn main() {}
