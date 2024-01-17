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

#[cfg(test)]
mod test_gen {
    use super::FCBBuf;
    use rand::prelude::*;

    impl<const CAPACITY: usize> FCBBuf<CAPACITY> {
        fn rand_slice_iter<R: Rng + Sized>(&self, rng: R) -> RandSliceIter<CAPACITY, R> {
            RandSliceIter {
                buffer: self,
                start: 0,
                rng,
            }
        }
    }

    struct RandSliceIter<'a, const CAPACITY: usize, R: Rng> {
        buffer: &'a FCBBuf<CAPACITY>,
        start: usize,
        rng: R,
    }

    impl<'a, const CAPACITY: usize, R: Rng> Iterator for RandSliceIter<'a, CAPACITY, R> {
        type Item = &'a [u8];

        fn next(&mut self) -> Option<Self::Item> {
            if self.start >= self.buffer.len {
                None
            } else {
                let end = self.start + self.rng.gen_range(1..=(self.buffer.len - self.start));
                let slice = &self.buffer.bytes[self.start..end];
                self.start = end;
                Some(slice)
            }
        }
    }
}

mod event {
    use rustix::{fd, io};
    use super::ReplicaID;

    // Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
    pub const MAX_SIZE: usize = 2048 * 1024;

    #[derive(bytemuck::Zeroable, bytemuck::Pod, Clone, Copy, Debug)]
    #[repr(C)]
    struct ID { origin: ReplicaID, pos: usize }

    impl ID {
        const LEN: usize = std::mem::size_of::<ID>();
    }

    #[derive(bytemuck::Zeroable, bytemuck::Pod, Clone, Copy, Debug)]
    #[repr(C)]
    struct Header { len: usize, origin: ReplicaID }

    impl Header {
        const SIZE: usize = std::mem::size_of::<Header>();
    }

    #[derive(Debug)]
    pub struct Event<'a> { id: ID, val: &'a [u8] }

    #[derive(Clone, Copy, Debug)]
    struct Index(usize);

    impl Index {
        fn new(byte_pos: usize) -> Self {
            if byte_pos % 8 != 0 {
                panic!("All indices must be 8 byte aligned")
            }

            Self(byte_pos)
        }

        fn as_usize(self) -> usize {
            self.0
        }
    }
    
    // 1..N events backed by a fixed capacity byte buffer
    // INVARIANTS
    // - starts at the start of an event
    // - ends at the end of an event
    // - aligns events to 8 bytes
    pub struct Buf{
        bytes: Vec<u8>,
        indices: Vec<Index>,
    }

    impl Buf {
        pub fn new() -> Buf {
            // TODO: deque of MAX_SIZE capacity byte buffers
            let bytes = Vec::with_capacity(MAX_SIZE);
            let indices = vec![];
            Self{bytes, indices}
        }

        pub fn append(&mut self, origin: ReplicaID, val: &[u8]) {
            let new_index = Index(self.len());
            let header = Header { len: ID::LEN + val.len(), origin };
            let new_len = self.bytes.len() + Header::SIZE + val.len();
            if new_len > self.bytes.capacity() { panic!("OVERFLOW") }

            let header = bytemuck::bytes_of(&header);
            self.bytes.extend_from_slice(header);
            self.bytes.extend_from_slice(val);

            assert_eq!(new_len, self.bytes.len());

            let aligned_new_len = (new_len + 7) & !7;
            self.bytes.resize(aligned_new_len, 0);
            // TODO: write padding so that the buffer len is multiples of 8
            assert_eq!(aligned_new_len, self.bytes.len());

            self.indices.push(new_index);
        }

        pub fn clear(&mut self) {
            self.bytes.clear()
        }

        fn get(&self, pos: usize) -> Option<Event> {
            if pos >= self.len() { return None; }

            let index = self.indices[pos];
            let byte_pos = index.as_usize();
            let header_range = byte_pos..byte_pos + Header::SIZE;

            let event_header: &Header =
                bytemuck::from_bytes(&self.bytes[header_range]);

            let val_range = 
                byte_pos + Header::SIZE .. byte_pos + event_header.len;

            let event = Event {
                id: ID { origin: event_header.origin, pos },
                val: &self.bytes[val_range]
            };

            Some(event)
        }

        pub fn len(&self) -> usize {
            self.indices.len()
        }

        pub fn append_to_file(&mut self, fd: fd::BorrowedFd) -> io::Result<usize> {
            // always sets file offset to EOF.
            let bytes_written = io::write(fd, &self.bytes)?;
            // Linux 'man open' says appending to file opened w/ O_APPEND is atomic
            // TODO: will this happen? if so how to recover?
            assert_eq!(bytes_written, self.bytes.len());
            Ok(bytes_written)
        }

        /*
        pub fn read_from_file(
           &mut self, fd: fd::BorrowedFd, index: &Index
        ) -> io::Result<()> {
            // need to set len so pread knows how much to fill
            if index.len > self.bytes.capacity() { panic!("OVERFLOW") }
            unsafe {
                self.bytes.set_len(index.len);
            }

            // pread ignores the fd offset, supply your own
            let bytes_read = io::pread(fd, &mut self.bytes, index.pos as u64)?;
            // If this isn't the case, we should figure out why!
            assert_eq!(bytes_read, index.len);

            Ok(())
        }
        */
    }

    pub struct BufIntoIterator<'a> {
        event_buf: &'a Buf,
        byte_index: usize
    }

    impl<'a> Iterator for BufIntoIterator<'a> {
        type Item = Event<'a>;

        fn next(&mut self) -> Option<Self::Item> {
            let result = self.event_buf.get(self.byte_index);

            if let Some(ref e) = result {
                self.byte_index += Header::SIZE + e.val.len();
            }

            result
        }
    }

    impl<'a> IntoIterator for &'a Buf {
        type Item = Event<'a>;
        type IntoIter = BufIntoIterator<'a>;

        fn into_iter(self) -> Self::IntoIter {
            BufIntoIterator {
                event_buf: self,
                byte_index: 0
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use proptest::prelude::*;
        use tempfile::TempDir;

        proptest! {
            #[test]
            fn read_and_write_single_events(
                e in prop::collection::vec(any::<u8>(), 0..=256)
            ) {
                // Setup 
                let mut rng = rand::thread_rng();
                let mut buf = Buf::new();
                let replica_id = ReplicaID::new(&mut rng);

                // Pre conditions
                assert_eq!(buf.len(), 0, "buf should start empty");
                assert!(buf.get(0).is_none(), "should contain no event");
               
                // Modifying
                buf.append(replica_id, &e);

                // Post conditions
                let actual = buf.get(0).expect("one event to be at 0");
                assert_eq!(buf.len(), 1);
                assert_eq!(actual.val, &e);
            }
        }

        #[test]
        fn read_and_write_to_log() {
            // Setup 
            let mut rng = rand::thread_rng();
            let mut buf = Buf::new();
            let replica_id = ReplicaID::new(&mut rng);

            // Pre conditions
            assert_eq!(buf.len(), 0, "buf should start empty");
            assert!(buf.get(0).is_none(), "should contain no event");

            let e1 = b"I've not grown weary on lenghty roads";
            let e2 = b"On strange paths, not gone astray";
            let e3 = b"Such is the knowledge, the knowledge cast in me";
            let e4 = b"Such is the knowledge; such are the skills";

            let es: [&[u8]; 4] = [
                e1.as_slice(),
                e2.as_slice(),
                e3.as_slice(),
                e4.as_slice()
            ];
            
            for e in es {
                buf.append(replica_id, e);
            }

            // Post conditions
            let actual: Vec<_> = 
                (0..4).map(|pos| buf.get(pos).unwrap().val).collect();
            assert_eq!(buf.len(), 4);
            assert_eq!(&actual, &es)

        }
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


pub struct LocalReplica {
    pub id: ReplicaID,
    pub path: std::path::PathBuf,
    log_fd: fd::OwnedFd,
    log_len: usize,
    write_cache: event::Buf,
    read_cache: event::Buf
}

/*
impl LocalReplica {
    pub fn new<R: Rng>(
        dir_path: &std::path::Path, rng: &mut R
    ) -> io::Result<Self> {
        let id = ReplicaID::new(rng);

        let path = dir_path.join(id.to_string());
        let flags = O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC;
        let mode = fs::Mode::RUSR | fs::Mode::WUSR;
		let log_fd = fs::open(&path, flags, mode)?;

		let log_len = 0;
        let indices = vec![];
        let write_cache = event::Buf::new();
        let read_cache = event::Buf::new();

		Ok(Self { id, path, log_fd, log_len, indices, write_cache, read_cache })
    }
    
    // Event local to the replica, that don't yet have an ID
    pub fn local_write(&mut self, datums: &[&[u8]]) -> io::Result<()> {
        // TODO: this is writing to the disk every loop..
        // TODO: does event buffer need its own indices??
        for data in datums {
            let header = event::Header {
                len: EventID::LEN + data.len(),
                origin: self.id,
            };

            self.write_cache.append(&header, data);

            let fd = self.log_fd.as_fd();
            let bytes_written = self.write_cache.append_to_file(fd)?;

            // Updating metadata
            let index = EventIndex { pos: self.log_len, len: header.len };
            self.indices.push(index);

            // round up to multiple of 8, for alignment
            self.log_len += (bytes_written + 7) & !7;
        }

        // Resetting
        self.write_cache.clear();

		Ok(())
	}
    
    // TODO: try to copy from read cache first before hitting disk
    pub fn read(&mut self, buf: &mut event::Buf, pos: usize) -> io::Result<()> {
        // TODO: reading from disk every loop
        // TODO: end condition??
        for index in &self.indices[pos..] {
            let fd = self.log_fd.as_fd();
            buf.read_from_file(fd, index)?;
        }

        Ok(())
    }
}
*/

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /*
    #[test]
	fn read_and_write_to_log() {
        let tmp_dir = 
            TempDir::with_prefix("interlog-").expect("failed to open temp file");

        let e1 = b"I've not grown weary on lenghty roads";
        let e2 = b"On strange paths, not gone astray";
        let e3 = b"Such is the knowledge, the knowledge cast in me";
        let e4 = b"Such is the knowledge; such are the skills";
		let mut rng = rand::thread_rng();
		let mut replica = LocalReplica::new(tmp_dir.path(), &mut rng)
            .expect("failed to open file");
		replica
            .local_write(&[e1, e2, e3, e4])
            .expect("failed to write to replica");

		let mut read_buf = event::Buf::new();
		replica.read(&mut read_buf, 0).expect("failed to read to file");
    
        let events: Vec<_> = read_buf.into_iter().collect();
        assert_eq!(events.len(), 4);
		assert_eq!(events[0].val, e1);
		let path = replica.path.clone();
		std::fs::remove_file(path).expect("failed to remove file");
    }
    */
}

fn main() {
}
