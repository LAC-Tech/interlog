#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes 64 bit");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

use std::io::Read;

use rand::prelude::*;
use rustix;

mod replica {
	use crate::rustix::{fs, io};
	use crate::Rng;
	use fs::OFlags;
	use std::collections::HashMap;

	#[repr(packed)]
	pub struct ID([u16; 5]);

	impl ID {
		fn new<R: Rng>(rng: &mut R) -> Self {
			Self(rng.gen())
		}
	}

	impl core::fmt::Display for ID {
		fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
			for n in self.0 {
				write!(f, "{:x}", n)?;
			}

			Ok(())
		}
	}

	#[repr(packed)]
	struct LogPos([u16; 3]);

	struct EventID {
		origin: ID,
		log_pos: LogPos,
	}

	pub struct Local {
		pub id: ID,
		pub path: std::path::PathBuf,
		// Stored in struct so drop will be called, closing the handle
		fd: rustix::fd::OwnedFd,
		id_index: HashMap<EventID, LogPos>,
	}

	impl Local {
		pub fn new<R: Rng>(rng: &mut R) -> Result<Self, io::Errno> {
			let id = ID::new(rng);
			let path_str = format!("/tmp/interlog/{}", id);
			let path = std::path::PathBuf::from(path_str);
			let fd = fs::open(
				&path,
				OFlags::DIRECT | OFlags::CREATE | OFlags::APPEND | OFlags::RDWR,
				fs::Mode::RUSR | fs::Mode::WUSR,
			)?;
			let id_index = HashMap::new();
			Ok(Self { id, path, fd, id_index })
		}

		pub fn write(&self, str: &str) -> Result<usize, io::Errno> {
			io::write(&self.fd, str.as_bytes())
		}
	}
}

fn main() {
	let mut rng = rand::thread_rng();
	let replica = replica::Local::new(&mut rng).expect("failed to open file");
	replica.write("Hello,\n").expect("failed to write to replica");
	replica.write("world!\n").expect("failed to write to replica");
	println!("Replica with id: {}", replica.id);
	println!("Press any key to continue");
	let _ = std::io::stdin().bytes().next();
	let path = replica.path.clone();
	std::fs::remove_file(path).expect("failed to remove file");
}
