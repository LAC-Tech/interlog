//! Interlog is a distributed, persistent log. It's append only and designed
//! to be a highly available source of truth for many different read models.
//!
//! The following implementation notes may be useful:
//! - Do the dumbest thing I can and test the hell out of it
//! - Allocate all memory at startup
//! - Direct I/O append only file, with KeyIndex that maps ID's to log offsets
//! - Storage engine Works at libc level (rustix), so you can follow man pages.
//! - Assumes linux, 64 bit, little endian - for now at least.
//! - Will orovide hooks to sync in the future, but actual HTTP (or whatever)
//! server is out of scope.

extern crate alloc;
mod disk;
mod fixvec;
mod log;
mod log_id;
mod test_utils;

pub mod event;
pub mod util;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");

#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");
