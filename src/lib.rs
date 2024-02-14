extern crate alloc;
mod disk;
mod event;
mod replica;
mod replica_id;
mod utils;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

pub use replica::*;
pub use replica_id::*;
pub use utils::FixVec;
pub use event::*;
