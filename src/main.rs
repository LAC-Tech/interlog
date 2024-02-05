extern crate alloc;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use fs::OFlags;

use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};

// Fixed Capacity Vector
// Tigerstyle: There IS a limit
struct FixVec<T> {
    elems: alloc::boxed::Box<[T]>,
    len: usize
}

enum FixVecErr { Overflow }
type FixVecRes = Result<(), FixVecErr>;

impl<T> FixVec<T> {
    #[inline]
    fn capacity(&self) -> usize {
        self.elems.len()
    }

    fn check_capacity(&self, new_len: usize) -> FixVecRes {
        if new_len > self.capacity() { 
            return Err(FixVecErr::Overflow);
        }

        Ok(())
    }

    fn push(&mut self, value: T) -> FixVecRes {
        let new_len = self.len + 1;
        self.check_capacity(new_len)?;
        self.elems[self.len] = value;
        self.len = new_len;
        Ok(())
    }

    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) -> FixVecRes {
        for elem in iter {
            self.push(elem)?;
        }

        Ok(())
    }

    fn get(&self, index: usize) -> Option<&T> {
        if self.len > index {
            Some(&self.elems[index])
        } else {
            None
        }
    }
}

impl<T: Clone + core::fmt::Debug> FixVec<T> {
    fn new(capacity: usize) -> FixVec<T> {
        let bytes: Box<[T]> = Vec::with_capacity(capacity).into_boxed_slice();
        assert_eq!(std::mem::size_of_val(&bytes), 16);
        let len = 0;

        Self {elems: bytes, len}
    }
    

    fn resize(&mut self, new_len: usize, value: T) {
        self.check_capacity(new_len);

        if new_len > self.len {
            self.elems[self.len..new_len].fill(value);
        }
        
        self.len = new_len;
    }

    #[inline]
    fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }
}

impl<T: Copy> FixVec<T> {
    fn extend_from_slice(&mut self, other: &[T]) {
        let new_len = self.len + other.len();
        if new_len > self.capacity() { panic!("overflow") }
        self.elems[self.len..new_len].copy_from_slice(other);
        self.len = new_len;
    }
}

impl<T> std::ops::Deref for FixVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.elems[0..self.len]
    }
}

mod event {
    use rustix::{fd, io};
    use super::{FixVec, ReplicaID};

    // Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
    pub const MAX_SIZE: usize = 2048 * 1024;

    #[repr(C)]
    #[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
    pub struct ID { origin: ReplicaID, pos: usize }

    impl ID {
        const SIZE: usize = std::mem::size_of::<Self>();
    }

    #[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
    #[repr(C)]
    struct Header { len: usize, origin: ReplicaID }

    impl Header {
        const SIZE: usize = std::mem::size_of::<Self>();
    }

    #[derive(Debug)]
    pub struct Event<'a> { pub id: ID, pub val: &'a [u8] }

    // 1..N events backed by a fixed capacity byte buffer
    // INVARIANTS
    // - starts at the start of an event
    // - ends at the end of an event
    // - aligns events to 8 bytes
    pub struct FixBuf {
        bytes: FixVec<u8>,
        indices: FixVec<usize>,
    }

    impl FixBuf {
        pub fn with_capacities(max_bytes: usize, max_indices: usize) -> Self {
            let bytes = FixVec::new(max_bytes); 
            let indices = FixVec::new(max_indices);
            Self{bytes, indices}
        }

        pub fn append(&mut self, origin: ReplicaID, val: &[u8]) {
            let new_index = self.bytes.len();
            let header = Header { len: ID::SIZE + val.len(), origin };
            let new_len = new_index + Header::SIZE + val.len();

            let header = bytemuck::bytes_of(&header);
            self.bytes.extend_from_slice(header);
            self.bytes.extend_from_slice(val);

            assert_eq!(new_len, self.bytes.len());

            let aligned_new_len = (new_len + 7) & !7;
            self.bytes.resize(aligned_new_len, 0);
            assert_eq!(aligned_new_len, self.bytes.len());

            self.indices.push(new_index.into());
        }

        pub fn extend(&mut self, other: &FixBuf) {
            let byte_len = self.bytes.len();
            self.indices.extend(other.indices.iter().map(|i| i + byte_len));
            self.bytes.extend_from_slice(&other.bytes);
        }

        pub fn clear(&mut self) {
            self.bytes.clear();
            self.indices.clear();
        }

        pub fn len(&self) -> usize {
            self.indices.len()
        }

        pub fn get(&self, pos: usize) -> Option<Event> {
            let index: usize = self.indices.get(pos).cloned()?.into();

            let header_range = index..index + Header::SIZE;
            
            let event_header: &Header =
                bytemuck::from_bytes(&self.bytes[header_range]);

            let val_range = index + Header::SIZE .. index + event_header.len;

            let event = Event {
                id: ID { origin: event_header.origin, pos },
                val: &self.bytes[val_range]
            };

            Some(event)
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

        pub fn append_to_file(&mut self, fd: fd::BorrowedFd) -> io::Result<usize> {
            // always sets file offset to EOF.
            let bytes_written = io::write(fd, &self.bytes)?;
            // Linux 'man open': appending to file opened w/ O_APPEND is atomic
            // TODO: will this happen? if so how to recover?
            assert_eq!(bytes_written, self.bytes.len());
            Ok(bytes_written)
        }
    }

    pub struct BufIntoIterator<'a> {
        event_buf: &'a FixBuf,
        index: usize
    }

    impl<'a> Iterator for BufIntoIterator<'a> {
        type Item = Event<'a>;

        fn next(&mut self) -> Option<Self::Item> {
            let result = self.event_buf.get(self.index);
            self.index += 1;
            result
        }
    }

    impl<'a> IntoIterator for &'a FixBuf {
        type Item = Event<'a>;
        type IntoIter = BufIntoIterator<'a>;

        fn into_iter(self) -> Self::IntoIter {
            BufIntoIterator { event_buf: self, index: 0 }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use proptest::prelude::*;
        use tempfile::TempDir;

        // TODO: too many allocations. Make a liffe vector implementation
        fn arb_byte_list(max: usize) -> impl Strategy<Value = Vec<Vec<u8>>> {
            proptest::collection::vec(
                proptest::collection::vec(any::<u8>(), 0..=max),
                0..=max
            )
        }

        proptest! {
            #[test]
            fn read_and_write_single_event(
                e in prop::collection::vec(any::<u8>(), 0..=8)
            ) {
                // Setup 
                let mut rng = rand::thread_rng();
                let mut buf = FixBuf::with_capacities(256, 8);
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

            #[test]
            fn multiple_read_and_write(es in arb_byte_list(16)) {
                // Setup 
                let mut rng = rand::thread_rng();
                let replica_id = ReplicaID::new(&mut rng);
                
                let mut buf = FixBuf::with_capacities(256, 8);

                // Pre conditions
                assert_eq!(buf.len(), 0, "buf should start empty");
                assert!(buf.get(0).is_none(), "should contain no event");
                
                for e in &es {
                    buf.append(replica_id, &e);
                }

                let len = es.len();

                // Post conditions
                assert_eq!(buf.len(), len);
                let actual: Vec<_> =
                    (0..len).map(|pos| buf.get(pos).unwrap().val).collect();
                assert_eq!(&actual, &es);
            }

            #[test]
            fn combine_buffers(es1 in arb_byte_list(16), es2 in arb_byte_list(16)) {
                // Setup 
                let mut rng = rand::thread_rng();
                let replica_id = ReplicaID::new(&mut rng);

                let mut buf1 = FixBuf::with_capacities(256, 16);  
                let mut buf2 = FixBuf::with_capacities(256, 16);  
                
                for e in &es1 {
                    buf1.append(replica_id, e);
                }

                for e in &es2 {
                    buf2.append(replica_id, e);
                }

                assert_eq!(buf1.len(), es1.len());
                assert_eq!(buf2.len(), es2.len());

                buf1.extend(&buf2);

                let actual: Vec<_> = 
                    (0..5).map(|pos| buf1.get(pos).unwrap().val).collect();

                let expected = es1.clone();

                assert_eq!(&actual, &expected);
            }
        }
    }
}

type O = OFlags;

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct EventID { origin: ReplicaID, pos: usize }

pub struct LocalReplica {
    pub id: ReplicaID,
    pub path: std::path::PathBuf,
    log_fd: fd::OwnedFd,
    log_len: usize,
    write_cache: event::FixBuf,
    read_cache: event::FixBuf
}

// TODO: store data larger than read cache
// TODO: mem cache larger than EVENT_MAX (ciruclar buffer?)
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
        let write_cache = event::FixBuf::with_capacities(256, 8);
        let read_cache = event::FixBuf::with_capacities(256, 8);

		Ok(Self { id, path, log_fd, log_len, write_cache, read_cache })
    }
    
    // Event local to the replica, that don't yet have an ID
    pub fn local_write(&mut self, datums: &[&[u8]]) -> io::Result<()> {
        for data in datums {
            self.write_cache.append(self.id, data);
        }
        
        // persist
        let fd = self.log_fd.as_fd();
        let bytes_written = self.write_cache.append_to_file(fd)?;

        // round up to multiple of 8, for alignment
        self.log_len += (bytes_written + 7) & !7;

        // Updating caches
        // TODO: should the below be combined to some 'drain' operation?
        assert_eq!(self.write_cache.len(), datums.len());
        self.read_cache.extend(&self.write_cache);
        self.write_cache.clear();

		Ok(())
	}
    
    pub fn read(&mut self, buf: &mut event::FixBuf, pos: usize) -> io::Result<()> {
        // TODO: check from disk if not in cache

        buf.extend(&self.read_cache);
        Ok(()) 
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
	fn read_and_write_to_log() {
        let tmp_dir = TempDir::with_prefix("interlog-")
            .expect("failed to open temp file");

        let es: [&[u8]; 4] = [
            b"I've not grown weary on lenghty roads",
            b"On strange paths, not gone astray",
            b"Such is the knowledge, the knowledge cast in me",
            b"Such is the knowledge; such are the skills"
        ];

		let mut rng = rand::thread_rng();
		let mut replica = LocalReplica::new(tmp_dir.path(), &mut rng)
            .expect("failed to open file");

		replica.local_write(&es).expect("failed to write to replica");

		let mut read_buf = event::FixBuf::with_capacities(256, 8);
		replica.read(&mut read_buf, 0).expect("failed to read to file");
   
        assert_eq!(read_buf.len(), 4);

        let events: Vec<_> = read_buf.into_iter().collect();
		assert_eq!(events[0].val, es[0]);
        assert_eq!(events.len(), 4);
		let path = replica.path.clone();
		std::fs::remove_file(path).expect("failed to remove file");
    }
}

fn main() {
    // TODO: test case I want to debug
    // Setup 
    let mut rng = rand::thread_rng();
    let replica_id = ReplicaID::new(&mut rng);

    let mut buf1 = event::FixBuf::with_capacities(256, 8);  
    let mut buf2 = event::FixBuf::with_capacities(256, 8);  
    
    let e1: &[u8] = b"Kan jy my skroewe vir my vasdraai?";
    let e2: &[u8] = b"Kan jy my albasters vir my vind?";
    let e3: &[u8] = b"Kan jy jou idee van normaal by jou gat opdruk?";
    let e4: &[u8] = b"Kan jy?";
    let e5: &[u8] = b"Kan jy 'apatie' spel?";

    let es = [e1, e2, e3, e4];

    for e in es {
        buf1.append(replica_id, e);
    }

    let expected = [e1, e2, e3, e4, e5];

    buf2.append(replica_id, e5);

    buf1.extend(&buf2);

    // Post conditions
    assert_eq!(buf1.len(), 5);
    let actual: Vec<_> = 
        (0..5).map(|pos| buf1.get(pos).unwrap().val).collect();
    assert_eq!(&actual, &expected);
}
