use fs::OFlags;

use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};
use crate::replica_id::ReplicaID;
use crate::utils::{CircBuf, CircBufWrapAround, FixVec, FixVecOverflow, Segment, unit};
use crate::{disk, event};

// Hugepagesize is "2048 kB" in /proc/meminfo. Assume kB = 1024
pub const MAX_SIZE: usize = 2048 * 1024;

type O = OFlags;

pub struct Config {
    pub index_capacity: usize,
    pub read_cache_capacity: unit::Byte,
    pub write_cache_capacity: unit::Byte
}

#[derive(Debug)]
pub enum WriteErr {
    Disk(disk::Err),
    ReadCache(CircBufWrapAround),
    WriteCache(FixVecOverflow),
    KeyIndex(FixVecOverflow)
}

#[derive(Debug)]
pub enum ReadErr {
    KeyIndex,
    ClientBuf(FixVecOverflow)
}

struct KeyIndex(FixVec<unit::Byte>);

impl KeyIndex {
    fn new(capacity: usize) -> Self {
        Self(FixVec::new(capacity))
    }
    
    fn len(&self) -> unit::Logical {
        self.0.len().into()
    }

    fn push(&mut self, byte_offset: unit::Byte) -> Result<(), WriteErr> {
        self.0.push(byte_offset).map_err(WriteErr::KeyIndex)
    }

    fn get(&self, logical_pos: unit::Logical) -> Result<unit::Byte, ReadErr> {
        let i: usize = logical_pos.into();
        self.0.get(i).cloned().ok_or(ReadErr::KeyIndex)
    }
    
    fn read_since(
        &self, pos: unit::Logical
    ) -> impl Iterator<Item=unit::Byte> + '_ {
        self.0.iter().skip(pos.into()).copied()
    }
}

/// ReadCache is a fixed sized structure that caches the latest entries in the
/// log (LIFO caching). The assumption is that things recently added are
/// most likely to be read out again.
///
/// To do this I'm using a single circular buffer, with two "write pointers".
/// At any given point in time the buffer will have two contiguous segments,
/// populated with events.
///
/// The reason to keep the two segments contiguous is so they can be easily
/// memcpy'd. So no event is split by the circular buffer.
///
/// There is a Top Segment, and a Bottom Segment.
/// 
/// We start with a top segment. The bottom segement gets created when we 
/// reach the end of the circular buffer and need to wrap around, eating into
/// the former top segment.
/// 
/// Example:
///
/// ┌---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┬---┐
/// | A | A | B | B | B |   | X | X | X | Y | Z | Z | Z | Z |   |   |
/// └---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┴---┘
/// 
/// The top segment contains events A and B, while the bottom segment contains
/// X, Y and Z
/// As more events are added, they will be appended after B, overwriting the
/// bottom segment, til it wraps round again.

struct ReadCache {
    buf: CircBuf<u8>,
    top: ReadCacheWritePtr,
    bottom: Option<ReadCacheWritePtr>
}

struct ReadCacheWritePtr {
    disk_offset: unit::Byte,
    segment: Segment
}

impl ReadCacheWritePtr {
    fn new() -> Self {
        Self { disk_offset: 0.into(), segment: Segment::new(0, 0) }
    }
}

impl ReadCache {
    fn new(capacity: unit::Byte) -> Self {
       let buf = CircBuf::new(capacity.into());
       let top = ReadCacheWritePtr::new();
       let bottom = None;
       Self {buf, top, bottom}
    }

    fn update(&mut self, write_cache: &[u8]) -> Result<(), WriteErr> {
        self.buf.extend_from_slice(write_cache).map_err(WriteErr::ReadCache)
    }

    fn read<O>(&self, disk_offsets: O) -> impl Iterator<Item = event::Event<'_>>
    where O: Iterator<Item = unit::Byte> {
        disk_offsets.map(|offset| event::read(&self.buf, offset)).fuse().flatten()
    }
}

pub struct Local {
    pub id: ReplicaID,
    pub path: std::path::PathBuf,
    log_fd: fd::OwnedFd,
    log_len: unit::Byte,
    write_cache: FixVec<u8>,
    read_cache: ReadCache,
    // The entire index in memory, like bitcask's KeyDir
    key_index: KeyIndex
}

// TODO: store data larger than read cache
impl Local {
    pub fn new<R: Rng>(
        dir_path: &std::path::Path, rng: &mut R, config: Config
    ) -> io::Result<Self> {
        let id = ReplicaID::new(rng);

        let path = dir_path.join(id.to_string());
        let flags = O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC;
        let mode = fs::Mode::RUSR | fs::Mode::WUSR;
		let log_fd = fs::open(&path, flags, mode)?;

		let log_len: unit::Byte = 0.into();
        let write_cache = FixVec::new(config.write_cache_capacity.into());
        // TODO: circular buffer
        let read_cache = ReadCache::new(config.read_cache_capacity);
        let key_index = KeyIndex::new(config.index_capacity); 

		Ok(Self { id, path, log_fd, log_len, write_cache, read_cache, key_index })
    }
    
    // Event local to the replica, that don't yet have an ID
    pub fn local_write<'a, I>(&mut self, datums: I) -> Result<(), WriteErr>
    where I: IntoIterator<Item = &'a[u8]> {
        self.write_cache.clear();
        let logical_start = self.key_index.len();
        
        self.write_cache
            .append_local_events(logical_start, self.id, datums)
            .map_err(WriteErr::WriteCache)?;
        
        // persist
        let fd = self.log_fd.as_fd();
        let bytes_written = disk::write(fd, &self.write_cache)
            .map_err(WriteErr::Disk)?;

        let bytes_written = bytes_written.align();

        // TODO: the below operations need to be made atomic w/ each other

        self.read_cache.update(&self.write_cache)?;

        let mut byte_offset = self.log_len;

        // TODO: event iterator implemented DIRECTLY on the read cache
        for e in self.read_cache.buf.into_iter() {
            self.key_index.push(byte_offset)?;
            byte_offset += e.on_disk_size();
        }

        assert_eq!(byte_offset - self.log_len, bytes_written);
        
        self.log_len += byte_offset;

        Ok(())
	}
    
    // TODO: assumes cache is 1:1 with disk
    pub fn read<P: Into<unit::Logical>>(
        &mut self, client_buf: &mut FixVec<u8>, pos: P 
    ) -> Result<(), ReadErr> { 
        let byte_offsets = self.key_index.read_since(pos.into());
        let events = self.read_cache.read(byte_offsets);
        client_buf.append_events(events).map_err(ReadErr::ClientBuf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ops::Deref;
    use tempfile::TempDir;
    use proptest::prelude::*;
    use pretty_assertions::assert_eq;
    use crate::test_utils::*;

    proptest! {
        #[test]
        fn rw_log(es in arb_byte_list(16)) {
            let tmp_dir = TempDir::with_prefix("interlog-")
                .expect("failed to open temp file");

            let mut rng = rand::thread_rng();
            let config = Config {
                index_capacity: 16,
                read_cache_capacity: unit::Byte(1024),
                write_cache_capacity: unit::Byte(1024)
            };

            let mut replica = Local::new(tmp_dir.path(), &mut rng, config)
                .expect("failed to open file");

            let vals = es.iter().map(Deref::deref);
            replica.local_write(vals)
                .expect("failed to write to replica");

            let mut read_buf = FixVec::new(0x400);
            replica.read(&mut read_buf, 0).expect("failed to read to file");
       
            let events: Vec<_> = read_buf.into_iter()
                .map(|e| e.val.to_vec())
                .collect();

            assert_eq!(events, es);
        }
    }
}
