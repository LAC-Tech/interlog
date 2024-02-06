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

type O = OFlags;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct EventID { origin: ReplicaID, pos: usize }

pub struct LocalReplicaConfig {
    pub read_cache_capacity: event::BufSize,
    pub write_cache_capacity: event::BufSize 
}

pub struct LocalReplica {
    pub id: ReplicaID,
    pub path: std::path::PathBuf,
    log_fd: fd::OwnedFd,
    log_len: usize,
    write_cache: event::FixBuf,
    read_cache: event::FixBuf
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

		let log_len = 0;
        let write_cache = event::FixBuf::new(config.write_cache_capacity);
        // TODO: circular buffer
        let read_cache = event::FixBuf::new(config.read_cache_capacity);

		Ok(Self { id, path, log_fd, log_len, write_cache, read_cache })
    }
    
    // Event local to the replica, that don't yet have an ID
    pub fn local_write(&mut self, datums: &[&[u8]]) -> Result<(), disk::Err> {
        assert_eq!(self.write_cache.len(), event::BufSize::ZERO);
        for data in datums {
            self.write_cache.append(self.id, data).expect("");
        }
        
        // persist
        let fd = self.log_fd.as_fd();

        let bytes_written = disk::write(fd, self.write_cache.as_bytes())?;

        // round up to multiple of 8, for alignment
        self.log_len += (bytes_written + 7) & !7;

        // Updating caches
        // TODO: should the below be combined to some 'drain' operation?
        assert_eq!(self.write_cache.len().indices, datums.len());
        self.read_cache.extend(&self.write_cache).expect("local write write cache");
        self.write_cache.clear();

		Ok(())
	}
    
    pub fn read(&mut self, buf: &mut event::FixBuf, pos: usize) -> io::Result<()> {
        // TODO: check from disk if not in cache

        buf.extend(&self.read_cache).expect("read read cache");
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
        let replica_config = LocalReplicaConfig {
            read_cache_capacity: event::BufSize::new(1024, 8),
            write_cache_capacity: event::BufSize::new(1024, 8)
        };
		let mut replica = LocalReplica::new(
            tmp_dir.path(),
            &mut rng,
            replica_config
        ).expect("failed to open file");

		replica.local_write(&es).expect("failed to write to replica");

		let mut read_buf = event::FixBuf::new(event::BufSize::new(0x200, 8));
		replica.read(&mut read_buf, 0).expect("failed to read to file");
   
        assert_eq!(read_buf.len().indices, 4);

        let events: Vec<_> = read_buf.into_iter().collect();
		assert_eq!(events[0].val, es[0]);
        assert_eq!(events.len(), 4);
    }
}

fn main() {
}
