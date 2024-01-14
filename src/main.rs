#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");


use fs::OFlags;

use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};

// TODO: all of these are pretty arbitrary
mod size {
    pub const EVENT_MAX: usize = 256;
}

// 1..N backed events backed by a fixed capacity byte buffer
struct EventBuf(Vec<u8>);

impl EventBuf {
    fn new(min_num_events: usize) -> EventBuf {
        Self(Vec::with_capacity(size::EVENT_MAX * min_num_events))
    }

    fn write(&mut self, header: &EventHeader, val: &[u8]) {
        let new_len = EventHeader::LEN + val.len();
        if new_len > self.0.capacity() { panic!("OVERFLOW") }

        let header = bytemuck::bytes_of(header);
        self.0.extend_from_slice(header);
        self.0.extend_from_slice(val);

        assert_eq!(new_len, self.0.len())
    }

    fn clear(&mut self) {
        self.0.clear()
    }

    // useful for rustix methods, which will fill buffers up to their lengths
    fn set_len(&mut self, new_len: usize) {
        if new_len > self.0.capacity() { panic!("OVERFLOW") }
        unsafe {
            self.0.set_len(new_len);
        }
    }

    // TODO: make iterator
    // We assume that 0 is always the start of an event
    fn event_at(&self, pos: usize) -> Event {
        let event_header: &EventHeader = 
            bytemuck::from_bytes(&self.0[pos..EventHeader::LEN]);

        Event {
            id: EventID { origin: event_header.origin, pos },
            val: &self[EventID::LEN..event_header.len]
        }
    }
}

struct EventBufIntoIterator<'a> {
    event_buf: &'a EventBuf,
    byte_index: usize
}

impl<'a> Iterator for EventBufIntoIterator<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
       None
    }
}

impl<'a> IntoIterator for &'a EventBuf {
    type Item = Event<'a>;
    type IntoIter = EventBufIntoIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        EventBufIntoIterator {
            event_buf: self,
            byte_index: 0
        }
    }
}

impl core::ops::Deref for EventBuf {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        let len = self.0.len();
        &self.0[0..len]
    }
}

impl core::ops::DerefMut for EventBuf {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let len = self.0.len();
        &mut self.0[0..len]
    }
}


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

pub struct LocalReplica {
    pub id: ReplicaID,
    pub path: String,
    log_fd: fd::OwnedFd,
    log_len: usize,
    indices: Vec<EventIndex>,
    write_cache: EventBuf,
    read_cache: EventBuf
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
        let write_cache = EventBuf::new(2);
        let read_cache = EventBuf::new(2);
		Ok(Self { id, path, log_fd, log_len, indices, write_cache, read_cache })
    }

    /*
    // Open existing replica
    pub fn open(path: &str) -> rustix::io::Result<Self> {
		let log_fd = Self::open_log(&path)?;
        let mut log_len: usize = 0;
        let mut event_offset = 0;
        let mut read_cache = EventBuf::new(2);

        loop {
            let bytes_read = 
                io::pread(log_fd.as_fd(), &mut read_cache, event_offset)?;

            log_len += bytes_read;

            let event_header: &EventHeader = 
                bytemuck::from_bytes(&read_cache[0..EventHeader::LEN]);
        }

        panic!("at the disco") 
    }
    */

    fn open_log(path: &str) -> rustix::io::Result<fd::OwnedFd> {
		fs::open(
			path,
			O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC,
			fs::Mode::RUSR | fs::Mode::WUSR,
		)
    }
    
    // Event local to the replica, that don't yet have an ID
    // TODO: write multiple things at a time
    pub fn local_write(&mut self, data: &[u8]) -> rustix::io::Result<()> {
        // Write to cache 
        
        let header = EventHeader {
            len: EventID::LEN + data.len(),
            origin: self.id,
        };
        self.write_cache.write(&header, data);

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
    
    // TODO: try to copy from read cache
    pub fn read(&mut self, buf: &mut EventBuf, pos: usize) -> rustix::io::Result<()> {
        let index = &self.indices[pos];
        buf.set_len(index.len);

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
	fn read_and_write_to_log() {
		let mut rng = rand::thread_rng();
		let mut replica = LocalReplica::new("/tmp/interlog", &mut rng)
            .expect("failed to open file");
		replica
            .local_write(b"Hello, world!\n")
            .expect("failed to write to replica");

		let mut read_buf = EventBuf::new(2);
		replica.read(&mut read_buf, 0).expect("failed to read to file");
    
        let event = read_buf.event_at(0);

		assert_eq!(&event.val, b"Hello, world!\n");
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

    let mut read_buf = EventBuf::new(2);
    replica.read(&mut read_buf, 0).expect("failed to read to file");

    for chunk in read_buf.chunks(8) {
        for byte in chunk {
            print!("{:02x} ", byte);
        }
        println!();
    }    
}
