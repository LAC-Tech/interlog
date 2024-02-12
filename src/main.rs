extern crate alloc;
mod disk;
mod event;
mod replica_id;
mod utils;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use fs::OFlags;

use rand::prelude::*;
use rustix::{fd, fd::AsFd, fs, io};

use replica_id::ReplicaID;

use crate::utils::{FixVec, FixVecErr};

type O = OFlags;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct EventID { origin: ReplicaID, pos: usize }

pub struct LocalReplicaConfig {
    pub index_capacity: usize,
    pub read_cache_capacity: usize,
    pub write_cache_capacity: usize
}

#[derive(Debug)]
pub enum LogicalReplicaErr {
    Disk(disk::Err),
    ReadCache(FixVecErr),
    WriteCache(FixVecErr),
    KeyIndex(FixVecErr)
}

type LogicalReplicaRes = Result<(), LogicalReplicaErr>;

pub struct LocalReplica {
    pub id: ReplicaID,
    pub path: std::path::PathBuf,
    log_fd: fd::OwnedFd,
    log_len: event::ByteOffset,
    write_cache: FixVec<u8>,
    read_cache: FixVec<u8>,
    // The entire index in memory, like bitcask's KeyDir
    key_index: FixVec<event::ByteOffset>
}

// TODO: store data larger than read cache
impl LocalReplica {
    pub fn new<R: Rng>(
        dir_path: &std::path::Path, rng: &mut R, config: LocalReplicaConfig
    ) -> io::Result<Self> {
        let id = ReplicaID::new(rng);

        let path = dir_path.join(id.to_string());
        let flags = O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC;
        let mode = fs::Mode::RUSR | fs::Mode::WUSR;
		let log_fd = fs::open(&path, flags, mode)?;

		let log_len: event::ByteOffset = 0.into();
        let write_cache = FixVec::new(config.write_cache_capacity);
        // TODO: circular buffer
        let read_cache = FixVec::new(config.read_cache_capacity);
        let key_index = FixVec::new(config.index_capacity); 

		Ok(Self { id, path, log_fd, log_len, write_cache, read_cache, key_index })
    }
    
    // Event local to the replica, that don't yet have an ID
    pub fn local_write(&mut self, datums: &[&[u8]]) -> LogicalReplicaRes {
        self.write_cache.clear();
        let logical_len = self.key_index.len();
        for (i, &data) in datums.into_iter().enumerate() {
            let logical_pos = logical_len + i;
            let id = event::ID { origin: self.id, logical_pos };
            self.write_cache.append_event(id, data)
                .map_err(LogicalReplicaErr::WriteCache);
        }
        
        // persist
        let fd = self.log_fd.as_fd();
        let bytes_written = disk::write(fd, &self.write_cache)
            .map_err(LogicalReplicaErr::Disk)?;


        // Updating caches
        self.read_cache.extend_from_slice(&self.write_cache)
            .map_err(LogicalReplicaErr::ReadCache)?;
        self.key_index.push(self.log_len).map_err(LogicalReplicaErr::KeyIndex)?;
        // round up to multiple of 8, for alignment
        self.log_len += event::ByteOffset((bytes_written + 7) & !7);

        Ok(())
	}
    
    pub fn read(
        &mut self, client_buf: &mut FixVec<u8>, logical_pos: usize
    ) -> io::Result<()> {
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
        let replica_config = LocalReplicaConfig {
            index_capacity: 16,
            read_cache_capacity: 1024,
            write_cache_capacity: 1024
        };
		let mut replica = LocalReplica::new(
            tmp_dir.path(),
            &mut rng,
            replica_config
        ).expect("failed to open file");

		replica.local_write(&es).expect("failed to write to replica");

        let mut read_buf = event::Buf::new(0x200);
		replica.read(&mut read_buf, 0).expect("failed to read to file");
   
        let events: Vec<_> = read_buf.into_iter().collect();
		assert_eq!(events[0].val, es[0]);
        assert_eq!(events.len(), 4);
    }
}

fn main() {
}
