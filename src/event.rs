use derive_more::*;
use crate::replica_id::ReplicaID;
use crate::utils::{FixVec, FixVecErr, FixVecRes, unit};

// Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
pub const MAX_SIZE: usize = 2048 * 1024;

// TODO: do I need to construct this oustide of this module?
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
pub struct ID { pub origin: ReplicaID, pub logical_pos: unit::Logical }

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
#[repr(C)]
struct Header { byte_len: usize, id: ID }

impl Header {
    const SIZE: usize = std::mem::size_of::<Self>();
    fn range(start: unit::Byte) -> core::ops::Range<usize> {
        start.0 .. start.0 + Self::SIZE
    }
}

#[derive(Clone, Debug)]
pub struct Event<'a> { pub id: ID, pub val: &'a [u8] }

impl<'a> Event<'a> {
    pub fn on_disk_size(&self) -> usize {
        Header::SIZE + self.val.len()
    }
}

/// 1..N events backed by a fixed capacity byte buffer
///
/// INVARIANTS
/// - starts at the start of an event
/// - ends at the end of an event
/// - aligns events to 8 bytes
impl FixVec<u8> {
    fn append_event(
        &mut self,
        start: unit::Byte,
        id: ID,
        val: &[u8]
    ) -> Result<unit::Byte, FixVecErr> {
        let offset = start + self.len().into();
        let header_range = Header::range(offset);
        let val_start = header_range.end;
        let byte_len = val.len();
        let val_range = val_start .. val_start + byte_len;
        let new_len = unit::Byte(val_range.end).align();
        self.resize(new_len.into(), 0)?;

        let header = Header { byte_len, id };
        let header = bytemuck::bytes_of(&header);

        self[header_range].copy_from_slice(header);
        self[val_range].copy_from_slice(val);

        Ok(offset)
    }

    pub fn append_events(
        &mut self,
        (logical_start, byte_start): (unit::Logical, unit::Byte),
        origin: ReplicaID,
        vals: &[&[u8]]
    ) -> Result<(), FixVecErr> {
        let mut byte_start = byte_start;

        for (i, &data) in vals.into_iter().enumerate() {
            let logical_pos = logical_start + i.into();
            let id = ID { origin, logical_pos };
            let bytes_appended = self.append_event(byte_start, id, data)?;
            byte_start += bytes_appended
        }

        Ok(())

    }

    pub fn read_event(&self, offset: unit::Byte) -> Option<Event<'_>> {
        let header_range = Header::range(offset);
        let val_start = header_range.end;
        let header_bytes = &self.get(header_range)?;
        let &Header {id, byte_len} = bytemuck::from_bytes(header_bytes);
        let val = &self[val_start..val_start + byte_len];
        Some(Event {id, val})
    }
}

pub struct BufIntoIterator<'a> {
    event_buf: &'a FixVec<u8>,
    index: unit::Byte
}

impl<'a> Iterator for BufIntoIterator<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.event_buf.read_event(self.index);
        self.index += result.clone()?.on_disk_size().into();
        result
    }
}

impl<'a> IntoIterator for &'a FixVec<u8> {
    type Item = Event<'a>;
    type IntoIter = BufIntoIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        BufIntoIterator { event_buf: self, index: 0.into() }
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
            let mut buf = FixVec::new(256);
            let replica_id = ReplicaID::new(&mut rng);

            // Pre conditions
            assert_eq!(buf.len(), 0, "buf should start empty");
            assert!(buf.get(0).is_none(), "should contain no event");
           
            // Modifying
            buf.append_event(replica_id, &e).expect("buf should have enough");

            // Post conditions
            let actual = buf.read_event(0.into()).expect("one event to be at 0");
            assert_eq!(buf.len(), 1);
            assert_eq!(actual.val, &e);
        }

        #[test]
        fn multiple_read_and_write(es in arb_byte_list(16)) {
            // Setup 
            let mut rng = rand::thread_rng();
            let replica_id = ReplicaID::new(&mut rng);
            
            let mut buf = FixVec::new(0x400);

            // Pre conditions
            assert_eq!(buf.len(), 0, "buf should start empty");
            assert!(buf.read_event(0.into()).is_none(), "should contain no event");
            
            for e in &es {
                buf.append_event(replica_id, &e).expect("buf should have enough");
            }

            let len = es.len();

            // Post conditions
            assert_eq!(buf.len(), len);
            let actual: Vec<_> = (0..len)
                .map(|pos| buf.read_event(pos.into()).unwrap().val)
                .collect();
            assert_eq!(&actual, &es);
        }

        #[test]
        fn combine_buffers(es1 in arb_byte_list(16), es2 in arb_byte_list(16)) {
            // Setup 
            let mut rng = rand::thread_rng();
            let replica_id = ReplicaID::new(&mut rng);

            let mut buf1 = FixVec::new(0x800); 
            let mut buf2 = FixVec::new(0x800);  
            
            for e in &es1 {
                buf1.append_event(replica_id, e).expect("buf should have enough");
            }

            for e in &es2 {
                buf2.append_event(replica_id, e).expect("buf should have enough");
            }

            assert_eq!(buf1.len(), es1.len());
            assert_eq!(buf2.len(), es2.len());

            buf1.extend_from_slice(&buf2).expect("buf should have enough");

            let actual: Vec<_> = (0..buf1.len())
                .map(|pos| buf1.read_event(pos.into()).unwrap().val)
                .collect();

            let mut expected = Vec::new();
            expected.extend(&es1);
            expected.extend(&es2);

            assert_eq!(&actual, &expected);
        }
    }
}
