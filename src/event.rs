use rustix::{fd, io};
use crate::replica_id::ReplicaID;
use crate::utils::{FixVec, FixVecErr};

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

#[derive(Debug)]
pub enum FixBufErr {
    Bytes(FixVecErr),
    Indices(FixVecErr)
}

pub type FixBufRes = Result<(), FixBufErr>;

impl FixBuf {
    pub fn with_capacities(max_bytes: usize, max_indices: usize) -> Self {
        let bytes = FixVec::new(max_bytes); 
        let indices = FixVec::new(max_indices);
        Self{bytes, indices}
    }

    pub fn append(&mut self, origin: ReplicaID, val: &[u8]) -> FixBufRes {
        let new_index = self.bytes.len();
        let header = Header { len: ID::SIZE + val.len(), origin };
        let new_len = new_index + Header::SIZE + val.len();

        let header = bytemuck::bytes_of(&header);
        self.bytes.extend_from_slice(header).map_err(FixBufErr::Bytes)?;
        self.bytes.extend_from_slice(val).map_err(FixBufErr::Bytes)?;

        assert_eq!(new_len, self.bytes.len());

        let aligned_new_len = (new_len + 7) & !7;
        self.bytes.resize(aligned_new_len, 0).map_err(FixBufErr::Bytes)?;
        assert_eq!(aligned_new_len, self.bytes.len());

        self.indices.push(new_index).map_err(FixBufErr::Indices)
    }

    pub fn extend(&mut self, other: &FixBuf) -> FixBufRes {
        let byte_len = self.bytes.len();
        self.indices.extend(other.indices.iter().map(|i| i + byte_len))
            .map_err(FixBufErr::Indices)?;
        self.bytes.extend_from_slice(&other.bytes)
            .map_err(FixBufErr::Bytes)
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
        self.indices.clear();
    }

    pub fn len(&self) -> usize {
        self.indices.len()
    }

    pub fn get(&self, pos: usize) -> Option<Event> {
        let index: usize = self.indices.get(pos).cloned()?;

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
            let mut buf = FixBuf::with_capacities(256, 1);
            let replica_id = ReplicaID::new(&mut rng);

            // Pre conditions
            assert_eq!(buf.len(), 0, "buf should start empty");
            assert!(buf.get(0).is_none(), "should contain no event");
           
            // Modifying
            buf.append(replica_id, &e).expect("buf should have enough");

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
            
            let mut buf = FixBuf::with_capacities(0x400, 16);

            // Pre conditions
            assert_eq!(buf.len(), 0, "buf should start empty");
            assert!(buf.get(0).is_none(), "should contain no event");
            
            for e in &es {
                buf.append(replica_id, &e).expect("buf should have enough");
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

            let mut buf1 = FixBuf::with_capacities(0x800, 32);  
            let mut buf2 = FixBuf::with_capacities(0x800, 32);  
            
            for e in &es1 {
                buf1.append(replica_id, e).expect("buf should have enough")
            }

            for e in &es2 {
                buf2.append(replica_id, e).expect("buf should have enough")
            }

            assert_eq!(buf1.len(), es1.len());
            assert_eq!(buf2.len(), es2.len());

            buf1.extend(&buf2).expect("buf should have enough");

            let actual: Vec<_> = (0..buf1.len())
                .map(|pos| buf1.get(pos).unwrap().val).collect();

            let mut expected = Vec::new();
            expected.extend(&es1);
            expected.extend(&es2);

            assert_eq!(&actual, &expected);
        }
    }
}
