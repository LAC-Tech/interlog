#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use fs::OFlags;
use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};

// Fixed Capacity Byte Buffers
struct FCBBuf<const CAPACITY: usize> {
    bytes: Box<[u8; CAPACITY]>,
    len: usize
}

impl<const CAPACITY: usize> FCBBuf<CAPACITY> {
    fn new() -> FCBBuf<CAPACITY> {
        let bytes = Box::new([0; CAPACITY]);
        let len = 0;

        Self {bytes, len}
    }

    fn extend_from_slice(&mut self, other: &[u8]) {
        let new_len = self.len + other.len();
        // TODO: proper option type
        if new_len > CAPACITY { panic!("OVERFLOW") }
        self.bytes[self.len..new_len].copy_from_slice(other);
        self.len = new_len;
    }

    fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> core::ops::Deref for FCBBuf<N> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &*self.bytes
    }
}

impl<const N: usize> core::ops::DerefMut for FCBBuf<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.bytes
    }
}


type O = OFlags;

// TODO: all of these are pretty arbitrary
mod size {
    pub const READ_CACHE: usize = 256;
    pub const WRITE_CACHE: usize = 256;
}

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

impl EventID {
    const LEN: usize = std::mem::size_of::<EventID>();
}

#[derive(bytemuck::Zeroable, bytemuck::Pod, Clone, Copy, Debug)]
#[repr(C)]
struct EventHeader { len: usize, origin: ReplicaID }

impl EventHeader {
    const LEN: usize = std::mem::size_of::<EventHeader>();
}

#[derive(Debug)]
struct Event<'a> { id: EventID, val: &'a [u8] }

// Including 'len' here to avoid two sys calls when reading event
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
            len: EventID::LEN + e.val.len(),
        };

        self.bytes.extend_from_slice(bytemuck::bytes_of(&e.id));
        self.bytes.extend_from_slice(e.val);

        self.indices.push(index);
    }

    fn read(&self, i: usize) -> Event {
        let index = &self.indices[i];
        let id_range = index.pos..index.pos + EventID::LEN;
        let val_range = index.pos + EventID::LEN..index.len;

        Event {
            id: *bytemuck::from_bytes(&self.bytes[id_range]),
            val: &self.bytes[val_range],
        }
    }
}

pub struct LocalReplica {
    pub id: ReplicaID,
    pub path: String,
    log_fd: fd::OwnedFd,
    log_len: usize,
    indices: Vec<EventIndex>,
    write_cache: FCBBuf<{ size::WRITE_CACHE }>,
    read_cache: FCBBuf<{ size::READ_CACHE }>
}

impl LocalReplica {
    pub fn new<R: Rng>(
        dir_path: &str, rng: &mut R
    ) -> rustix::io::Result<Self> {
        let id = ReplicaID::new(rng);
        let path = format!("{}/{}", dir_path, id);
		let log_fd = Self::open_log(&path)?;
		let log_len = 0;
        let indices = vec![];
        let write_cache = FCBBuf::new();
        let read_cache = FCBBuf::new();
		Ok(Self { id, path, log_fd, log_len, indices, write_cache, read_cache })
    }

    // Open existing replica
    pub fn open(path: &str) -> rustix::io::Result<Self> {
		let log_fd = Self::open_log(&path)?;
        let mut log_len: usize = 0;
        let mut read_cache: FCBBuf<{ size::READ_CACHE }> = FCBBuf::new();

        loop {
            let bytes_read =
                io::pread(log_fd.as_fd(), &mut read_cache, log_len as u64)?;

            log_len += bytes_read;

            // TODO: read bytes and update indices
            

            if bytes_read > size::READ_CACHE { break }
        }

        panic!("at the disco") 
    }

    fn open_log(path: &str) -> rustix::io::Result<fd::OwnedFd> {
		fs::open(
			path,
			O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC,
			fs::Mode::RUSR | fs::Mode::WUSR,
		)
    }
    
    // Event local to the replica, that don't yet have an ID
    pub fn local_write(&mut self, data: &[u8]) -> rustix::io::Result<()> {
        // Write to cache 
        
        let header = EventHeader {
            len: EventID::LEN + data.len(),
            origin: self.id,
        };
        self.write_cache.extend_from_slice(bytemuck::bytes_of(&header));
        self.write_cache.extend_from_slice(data); 

        // always sets file offset to EOF.
		let bytes_written = io::write(self.log_fd.as_fd(), &self.write_cache)?;
		// Linux 'man open' says appending to file opened w/ O_APPEND is atomic
		assert_eq!(bytes_written, self.write_cache.len());

        // Updating metadata
        let index = EventIndex {
            pos: self.log_len,
            len: header.len 
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
		let mut replica = LocalReplica::new("/tmp/interlog", &mut rng)
            .expect("failed to open file");
		replica
            .local_write(b"Hello, world!\n")
            .expect("failed to write to replica");

		let mut read_buf: Vec<u8> = Vec::with_capacity(512);
		replica.read(&mut read_buf, 0).expect("failed to read to file");
    
        let actual_event_header: &EventHeader = 
            bytemuck::from_bytes(&read_buf[0..EventHeader::LEN]);

        let actual_event = Event {
            id: EventID { origin: actual_event_header.origin, pos: 0 },
            val: &read_buf[EventID::LEN..]
        };

		assert_eq!(&actual_event.val, b"Hello, world!\n");
		let path = replica.path.clone();
		std::fs::remove_file(path).expect("failed to remove file");
    }
}

fn main() {
    let mut rng = rand::thread_rng();
    let mut replica = LocalReplica::new("/tmp/interlog", &mut rng)
        .expect("failed to open file");
    replica
        .local_write(b"Who is this doin' this synthetic type of alpha beta psychedelic funkin'?")
        .expect("failed to write to replica");

    let mut read_buf: Vec<u8> = Vec::with_capacity(512);
    replica.read(&mut read_buf, 0).expect("failed to read to file");

    for chunk in read_buf.chunks(8) {
        for byte in chunk {
            print!("{:02x} ", byte);
        }
        println!();
    }    
}
