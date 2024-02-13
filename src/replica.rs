use fs::OFlags;

use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};
use crate::replica_id::ReplicaID;
use crate::utils::{FixVec, FixVecOverflow, unit};
use crate::{disk, event};

type O = OFlags;

pub struct Config {
    pub index_capacity: usize,
    pub read_cache_capacity: usize,
    pub write_cache_capacity: usize
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
}

pub struct Local {
    pub id: ReplicaID,
    pub path: std::path::PathBuf,
    log_fd: fd::OwnedFd,
    log_len: unit::Byte,
    write_cache: FixVec<u8>,
    read_cache: FixVec<u8>,
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
        let write_cache = FixVec::new(config.write_cache_capacity);
        // TODO: circular buffer
        let read_cache = FixVec::new(config.read_cache_capacity);
        let key_index = KeyIndex::new(config.index_capacity); 

		Ok(Self { id, path, log_fd, log_len, write_cache, read_cache, key_index })
    }
    
    // Event local to the replica, that don't yet have an ID
    pub fn local_write(&mut self, datums: &[&[u8]]) -> Result<(), WriteErr> {
        self.write_cache.clear();
        let logical_start: unit::Logical = self.key_index.len().into();
        
        self.write_cache
            .append_events((logical_start, self.log_len), self.id, datums)
            .map_err(WriteErr::WriteCache)?;
        
        // persist
        let fd = self.log_fd.as_fd();
        let bytes_written =  disk::write(fd, &self.write_cache)
            .map_err(WriteErr::Disk)?;

        let bytes_written = bytes_written.align();

        // TODO: the below operations need to be made atomic w/ each other

        // Updating caches
        self.read_cache.extend_from_slice(&self.write_cache)
            .map_err(WriteErr::ReadCache)?;

        let mut byte_offset = self.log_len;

        for e in self.read_cache.into_iter() {
            self.key_index.push(byte_offset)?;
            byte_offset += e.on_disk_size().into();
        }

        assert_eq!(byte_offset - self.log_len, bytes_written);
        
        self.log_len += byte_offset;


        Ok(())
	}
    
    pub fn read(
        &mut self, client_buf: &mut FixVec<u8>, logical_pos: usize
    ) -> Result<(), ReadErr> {
        let disk_pos = self.key_index.get(logical_pos.into())?;

        

        panic!("first look up key in index. if it exists, check cache. then check disk")
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
        let config = Config {
            index_capacity: 16,
            read_cache_capacity: 1024,
            write_cache_capacity: 1024
        };
		let mut replica = Local::new(
            tmp_dir.path(),
            &mut rng,
            config
        ).expect("failed to open file");

		replica.local_write(&es).expect("failed to write to replica");

        let mut read_buf = FixVec::new(0x200);
		replica.read(&mut read_buf, 0).expect("failed to read to file");
   
        let events: Vec<_> = read_buf.into_iter().collect();
		assert_eq!(events[0].val, es[0]);
        assert_eq!(events.len(), 4);
    }
}
