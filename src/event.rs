use crate::replica_id::ReplicaID;
use crate::utils::{unit, FixVec, FixVecOverflow};

// Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
pub const MAX_SIZE: usize = 2048 * 1024;

// TODO: do I need to construct this oustide of this module?
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
pub struct ID {
    pub origin: ReplicaID,
    pub pos: unit::Logical,
}

impl ID {
    pub fn new<P: Into<unit::Logical>>(origin: ReplicaID, disk_pos: P) -> Self {
        ID {
            origin,
            pos: disk_pos.into(),
        }
    }
}

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
#[repr(C)]
struct Header {
    byte_len: usize,
    id: ID,
}

impl Header {
    const SIZE: usize = std::mem::size_of::<Self>();
    fn range(start: unit::Byte) -> core::ops::Range<usize> {
        start.0..start.0 + Self::SIZE
    }
}

#[derive(Clone, Debug)]
pub struct Event<'a> {
    pub id: ID,
    pub val: &'a [u8],
}

impl<'a> Event<'a> {
    pub fn on_disk_size(&self) -> unit::Byte {
        let size: unit::Byte = (Header::SIZE + self.val.len()).into();
        size.align()
    }
}

/// 1..N events backed by a fixed capacity byte buffer
///
/// INVARIANTS
/// - starts at the start of an event
/// - ends at the end of an event
/// - aligns events to 8 bytes
impl FixVec<u8> {
    // TODO: make private, only public to debug test
    pub fn append_event(
        &mut self,
        event: &Event
    ) -> Result<(), FixVecOverflow> {
        dbg!(event);
        let Event { id, val } = *event;
        let offset = self.len().into();
        let header_range = Header::range(offset);
        let val_start = header_range.end;
        let byte_len = val.len();
        let val_range = val_start..val_start + byte_len;
        let new_len = unit::Byte(val_range.end).align();
        self.resize(new_len.into(), 0)?;

        let header = Header { byte_len, id };
        let header = bytemuck::bytes_of(&header);

        dbg!(&header_range);
        dbg!(&val_range);
        self[header_range].copy_from_slice(header);
        self[val_range].copy_from_slice(val);


        Ok(())
    }

    pub fn append_local_events<'a, I>(
        &mut self,
        start: unit::Logical,
        origin: ReplicaID,
        vals: I,
    ) -> Result<(), FixVecOverflow>
    where
        I: IntoIterator<Item = &'a [u8]>,
    {
        for (i, val) in vals.into_iter().enumerate() {
            let pos = start + i.into();
            let id = ID { origin, pos };
            let e = Event {id, val};
            self.append_event(&e)?;
        }

        Ok(())
    }

    pub fn append_events<'a, I>(
        &mut self,
        events: I,
    ) -> Result<(), FixVecOverflow>
    where
        I: IntoIterator<Item = Event<'a>>,
    {
        for e in events {
            self.append_event(&e)?;
        }

        Ok(())
    }

    pub fn read_event(&self, offset: unit::Byte) -> Option<Event<'_>> {
        let header_range = Header::range(offset);
        dbg!(&header_range);
        let val_start = header_range.end;
        let header_bytes = &self.get(header_range)?;
        let &Header { id, byte_len } = bytemuck::from_bytes(header_bytes);
        dbg!(id);
        dbg!(byte_len);
        let val_end = val_start + byte_len;
        let val_range = val_start .. val_end;
        dbg!(&val_range);
        //if val_start == val_end && val_end == self.len() { return None }
        let val = &self.get(val_range)?;
        let event = Event { id, val };
        dbg!(&event);
        Some(event)
    }
}

pub struct BufIntoIterator<'a> {
    event_buf: &'a FixVec<u8>,
    index: unit::Byte,
}

impl<'a> Iterator for BufIntoIterator<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        dbg!(self.index);
        let result = self.event_buf.read_event(self.index)?;
        self.index += result.clone().on_disk_size().into();
        Some(result)
    }
}

impl<'a> IntoIterator for &'a FixVec<u8> {
    type Item = Event<'a>;
    type IntoIter = BufIntoIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        BufIntoIterator {
            event_buf: self,
            index: 0.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ops::Deref;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use tempfile::TempDir;

    // TODO: too many allocations. Make a liffe vector implementation
    fn arb_byte_list(max: usize) -> impl Strategy<Value = Vec<Vec<u8>>> {
        proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..=max), 0..=max)
    }

    proptest! {
        #[test]
        fn rw_single_event(
            e in prop::collection::vec(any::<u8>(), 0..=8)
        ) {
            // Setup
            let mut rng = rand::thread_rng();
            let mut buf = FixVec::new(256);
            let replica_id = ReplicaID::new(&mut rng);
            let event = Event {id: ID::new(replica_id, 0), val: &e};

            // Pre conditions
            assert_eq!(buf.len(), 0, "buf should start empty");
            assert!(buf.get(0).is_none(), "should contain no event");

            println!("\nAPPEND\n");
            // Modifying
            buf.append_event(&event).expect("buf should have enough");

            println!("\nREAD\n");
            // Post conditions
            let actual = buf.read_event(0.into())
                .expect("one event to be at 0");
            assert_eq!(actual.val, &e);
        }

        #[test]
        fn rw_many_events(es in arb_byte_list(16)) {
            // Setup
            let mut rng = rand::thread_rng();
            let replica_id = ReplicaID::new(&mut rng);

            let mut buf = FixVec::new(0x400);

            // Pre conditions
            assert_eq!(buf.len(), 0, "buf should start empty");
            assert!(buf.read_event(0.into()).is_none(), "should contain no event");

            let vals = es.iter().map(Deref::deref);
            buf.append_local_events(0.into(), replica_id, vals)
                .expect("buf should have enough");

            // Post conditions
            let actual: Vec<_> = buf.into_iter().map(|e| e.val).collect();
            assert_eq!(&actual, &es);
        }

        #[test]
        fn combine_buffers(es1 in arb_byte_list(16), es2 in arb_byte_list(16)) {
            // Setup
            let mut rng = rand::thread_rng();
            let replica_id = ReplicaID::new(&mut rng);

            let mut buf1 = FixVec::new(0x800);
            let mut buf2 = FixVec::new(0x800);

            let start: unit::Logical = 0.into();

            buf1.append_local_events(start, replica_id, es1.iter().map(Deref::deref))
                .expect("buf should have enough");

            buf2.append_local_events(start, replica_id, es2.iter().map(Deref::deref))
                .expect("buf should have enough");

            buf1.extend_from_slice(&buf2).expect("buf should have enough");

            let actual: Vec<_> = buf1.into_iter().map(|e| e.val).collect();

            let mut expected = Vec::new();
            expected.extend(&es1);
            expected.extend(&es2);

            assert_eq!(&actual, &expected);
        }
    }
}
