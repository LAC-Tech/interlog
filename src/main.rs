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
#[repr(transparent)]
pub struct ReplicaID(u128);

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

#[derive(bytemuck::Zeroable, bytemuck::Pod, Clone, Copy, Debug)]
#[repr(C)]
struct EventID { origin: ReplicaID, pos: usize }
const EVENT_ID_LEN: usize = std::mem::size_of::<EventID>();

#[derive(Debug)]
struct Event<'a> { id: EventID, val: &'a [u8] }

struct EventIndex { pos: usize, len: usize }

// TODO: there's some confusion with this struct
// Is it a buffer? or an immutable collection of events?
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

enum WriteErr {}

type WriteRes<L> = Result<L, WriteErr>;

pub struct LocalReplica {
    pub id: ReplicaID,
    pub path: std::path::PathBuf,
    log_fd: fd::OwnedFd,
    log_len: usize,
    indices: Vec<EventIndex>,
    write_cache: Vec<u8>,
}

impl LocalReplica {
    fn new<R: Rng>(rng: &mut R) -> rustix::io::Result<Self> {
        let id = ReplicaID::new(rng);
		let path_str = format!("/tmp/interlog/{}", id);
		let path = std::path::PathBuf::from(path_str);
		let log_fd = fs::open(
			&path,
			O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC,
			fs::Mode::RUSR | fs::Mode::WUSR,
		)?;
		let log_len = 0;
        let indices = vec![];
        let write_cache = vec![];
		Ok(Self { id, path, log_fd, log_len, indices, write_cache })
    }
    
    // Event local to the replica, that don't yet have an ID
    pub fn local_write(&mut self, data: &[u8]) -> rustix::io::Result<()> {
        // Write to cache 
        let id = EventID { origin: self.id, pos: self.indices.len() };
        self.write_cache.extend_from_slice(bytemuck::bytes_of(&id));
		self.write_cache.extend_from_slice(data);

        // always sets file offset to EOF.
		let bytes_written = io::write(self.log_fd.as_fd(), &self.write_cache)?;
		// Linux 'man open' says appending to file opened w/ O_APPEND is atomic
		assert_eq!(bytes_written, self.write_cache.len());

        // Updating metadata
        let index = EventIndex {
            pos: self.log_len,
            len: self.write_cache.len()
        };
        self.indices.push(index);
		self.log_len += bytes_written;
		// Resetting
		self.write_cache.clear();

		Ok(())
	}

    pub fn read(
        &mut self, buf: &mut Vec<u8>, pos: usize
    ) -> rustix::io::Result<()> {
        let index = &self.indices[pos];
        unsafe {
            buf.set_len(index.len)
        }
        // pread ignores the fd offset, supply your own
        let bytes_read = io::pread(self.log_fd.as_fd(), buf, index.pos as u64)?;
        
        // If this isn't the case, we should figure out why!
        assert_eq!(bytes_read, index.len);

        Ok(())
    }
}


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
            id: EventID { origin: ReplicaID(0), pos: 0 },
            val: bytes,
        };

        let mut es = EventBuffer::new(); 

        es.write(e);
        let actual = es.read(0);

        assert_eq!(val, *bytemuck::from_bytes(actual.val))
    }
    
    #[test]
	fn read_and_write_to_log() {
		let mut rng = rand::thread_rng();
		let mut replica = LocalReplica::new(&mut rng)
            .expect("failed to open file");
		replica
            .local_write(b"Hello, world!\n")
            .expect("failed to write to replica");

		let mut read_buf: Vec<u8> = Vec::with_capacity(512);
		replica.read(&mut read_buf, 0).expect("failed to read to file");
    
        let actual_event_id: &EventID = 
            bytemuck::from_bytes(&read_buf[0..EVENT_ID_LEN]);

        let actual_event = Event {
            id: *actual_event_id,
            val: &read_buf[EVENT_ID_LEN..]
        };

		assert_eq!(&actual_event.val, b"Hello, world!\n");
		let path = replica.path.clone();
		std::fs::remove_file(path).expect("failed to remove file");
    }
}

fn main() {
    let mut rng = rand::thread_rng();
    let mut replica = LocalReplica::new(&mut rng)
        .expect("failed to open file");
    replica
        .local_write(b"Who is this doin' this synthetic type of alpha beta psychedelic funkin'?")
        .expect("failed to write to replica");

    let mut read_buf: Vec<u8> = Vec::with_capacity(512);
    replica.read(&mut read_buf, 0).expect("failed to read to file");

    dbg!(read_buf);
}
