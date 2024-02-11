use derive_more::*;
use crate::replica_id::ReplicaID;
use crate::utils::{FixVec, FixVecErr};

// Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
pub const MAX_SIZE: usize = 2048 * 1024;

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
pub struct ID { origin: ReplicaID, logical_pos: usize }

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
#[repr(C)]
struct Header { byte_len: usize, origin: ReplicaID }

impl Header {
    const SIZE: usize = std::mem::size_of::<Self>();
}

#[derive(Debug)]
pub struct Event<'a> { pub id: ID, pub val: &'a [u8] }

/// Represents a byte address, divisible by 8, where an Event starts
#[repr(transparent)]
#[derive(Add, Clone, Copy)]
struct ByteOffset(usize);

impl ByteOffset {
    fn header_range(self) -> core::ops::Range<usize> {
        self.0 .. self.0 + Header::SIZE
    }
}

impl FixVec<u8> {
    fn append_event(
        &mut self, 
        origin: ReplicaID,
        val: &[u8]
    ) -> Result<ByteOffset, FixVecErr> {
        let offset = ByteOffset(self.len());
        let header_range = offset.header_range();
        let val_start = header_range.end;
        let byte_len = val.len();
        let val_range = val_start .. val_start + byte_len;
        let new_len = (val_range.end + 7) & !7;
        self.resize(new_len, 0)?;

        let header = Header { byte_len, origin };
        let header = bytemuck::bytes_of(&header);

        self[header_range].copy_from_slice(header);
        self[val_range].copy_from_slice(val);

        Ok(offset)
    }

    fn read_event(&self, offset: ByteOffset, logical_pos: usize) -> Event<'_> {
        let header_range = offset.header_range();
        let val_start = header_range.end;
        let header_bytes = &self[header_range];
        let &Header {origin, byte_len} = bytemuck::from_bytes(header_bytes);
        let id = ID { origin, logical_pos };
        let val = &self[val_start..val_start + byte_len];
        Event {id, val}
    }
}

// Can be used for both Fixed and Circular?
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BufSize {
    pub bytes: usize,
    pub logical: usize
}

impl BufSize {
    pub const fn new(bytes: usize, logical: usize) -> Self {
        Self {bytes, logical}
    }

    pub const ZERO: BufSize = BufSize::new(0, 0);
}

/// 1..N events backed by a fixed capacity byte buffer
///
/// INVARIANTS
/// - starts at the start of an event
/// - ends at the end of an event
/// - aligns events to 8 bytes
pub struct FixBuf {
    bytes: FixVec<u8>,
    /// Maps logical positions to byte offsets
    offsets: FixVec<ByteOffset>,
}

#[derive(Debug)]
pub enum FixBufErr {
    Bytes(FixVecErr),
    Indices(FixVecErr)
}

pub type FixBufRes = Result<(), FixBufErr>;

impl FixBuf {
    pub fn new(capacity: BufSize) -> Self {
        let bytes = FixVec::new(capacity.bytes); 
        let offsets = FixVec::new(capacity.logical);
        Self{bytes, offsets}
    }

    // TODO: bulk append: calc length upfront and resize once
    pub fn append(&mut self, origin: ReplicaID, val: &[u8]) -> FixBufRes {
        let new_offset = self.bytes.append_event(origin, val)
            .map_err(FixBufErr::Bytes)?;

        self.offsets.push(new_offset).map_err(FixBufErr::Indices)
    }

    pub fn extend(&mut self, other: &FixBuf, logical_pos: usize) -> FixBufRes {
        let offset = ByteOffset(self.bytes.len());
        let remapped_indices = 
            other.offsets.iter().skip(logical_pos).map(|&i| i + offset);
        self.offsets.extend(remapped_indices).map_err(FixBufErr::Indices)?;
        // TODO: undo the extension at this point? should be atomic
        self.bytes.extend_from_slice(&other.bytes).map_err(FixBufErr::Bytes)
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
        self.offsets.clear();
    }

    pub fn len(&self) -> BufSize {
        BufSize::new(self.bytes.len(), self.offsets.len())
    }

    pub fn get(&self, logical_pos: usize) -> Option<Event> {
        let offset = self.offsets.get(logical_pos).cloned()?;
        Some(self.bytes.read_event(offset, logical_pos))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

pub struct BufIntoIterator<'a> {
    event_buf: &'a FixBuf,
    logical_index: usize
}

impl<'a> Iterator for BufIntoIterator<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.event_buf.get(self.logical_index);
        self.logical_index += 1;
        result
    }
}

impl<'a> IntoIterator for &'a FixBuf {
    type Item = Event<'a>;
    type IntoIter = BufIntoIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        BufIntoIterator { event_buf: self, logical_index: 0 }
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
            let mut buf = FixBuf::new(BufSize::new(256, 1));
            let replica_id = ReplicaID::new(&mut rng);

            // Pre conditions
            assert_eq!(buf.len(), BufSize::ZERO, "buf should start empty");
            assert!(buf.get(0).is_none(), "should contain no event");
           
            // Modifying
            buf.append(replica_id, &e).expect("buf should have enough");

            // Post conditions
            let actual = buf.get(0).expect("one event to be at 0");
            assert_eq!(buf.len().logical, 1);
            assert_eq!(actual.val, &e);
        }

        #[test]
        fn multiple_read_and_write(es in arb_byte_list(16)) {
            // Setup 
            let mut rng = rand::thread_rng();
            let replica_id = ReplicaID::new(&mut rng);
            
            let mut buf = FixBuf::new(BufSize::new(0x400, 16));

            // Pre conditions
            assert_eq!(buf.len(), BufSize::ZERO, "buf should start empty");
            assert!(buf.get(0).is_none(), "should contain no event");
            
            for e in &es {
                buf.append(replica_id, &e).expect("buf should have enough");
            }

            let len = es.len();

            // Post conditions
            assert_eq!(buf.len().logical, len);
            let actual: Vec<_> =
                (0..len).map(|pos| buf.get(pos.into()).unwrap().val).collect();
            assert_eq!(&actual, &es);
        }

        #[test]
        fn combine_buffers(es1 in arb_byte_list(16), es2 in arb_byte_list(16)) {
            // Setup 
            let mut rng = rand::thread_rng();
            let replica_id = ReplicaID::new(&mut rng);

            let mut buf1 = FixBuf::new(BufSize::new(0x800, 32));  
            let mut buf2 = FixBuf::new(BufSize::new(0x800, 32));  
            
            for e in &es1 {
                buf1.append(replica_id, e).expect("buf should have enough")
            }

            for e in &es2 {
                buf2.append(replica_id, e).expect("buf should have enough")
            }

            assert_eq!(buf1.len().logical, es1.len());
            assert_eq!(buf2.len().logical, es2.len());

            buf1.extend(&buf2, 0).expect("buf should have enough");

            let actual: Vec<_> = (0..buf1.len().logical)
                .map(|pos| buf1.get(pos.into()).unwrap().val).collect();

            let mut expected = Vec::new();
            expected.extend(&es1);
            expected.extend(&es2);

            assert_eq!(&actual, &expected);
        }
    }
}
