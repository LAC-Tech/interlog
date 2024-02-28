extern crate alloc;
mod disk;

mod test_utils;

pub mod event;
pub mod replica;
pub mod replica_id;
pub mod utils;
pub mod unit;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");
