use fs::OFlags;

use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};
use crate::replica_id::ReplicaID;
use crate::utils::{FixVec, FixVecOverflow, unit};
use crate::{disk, event};

type O = OFlags;

pub struct Config {
    pub index_capacity: usize,
    pub read_cache_capacity: unit::Byte,
    pub write_cache_capacity: unit::Byte
}

#[derive(Debug)]
pub enum WriteErr {
    Disk(disk::Err),
    ReadCache(FixVecOverflow),
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

struct ReadCache(FixVec<u8>);

impl ReadCache {
    fn new(capacity: unit::Byte) -> Self {
       Self(FixVec::new(capacity.into())) 
    }

    fn update(&mut self, write_cache: &[u8]) -> Result<(), WriteErr> {
        self.0.extend_from_slice(write_cache).map_err(WriteErr::ReadCache)
    }

    fn read<O>(&self, offsets: O) -> impl Iterator<Item = event::Event<'_>>
    where O: Iterator<Item = unit::Byte> {
        offsets.map(|offset| self.0.read_event(offset)).fuse().flatten()
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
        let bytes_written =  disk::write(fd, &self.write_cache)
            .map_err(WriteErr::Disk)?;

        let bytes_written = bytes_written.align();

        // TODO: the below operations need to be made atomic w/ each other

        self.read_cache.update(&self.write_cache)?;

        let mut byte_offset = self.log_len;

        for e in self.read_cache.0.into_iter() {
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
    use crate::test_utils::*;

    proptest! {
        #[test]
        fn read_and_write_to_log(es in arb_byte_list(16)) {
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
