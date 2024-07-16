//! Interlog is a distributed, persistent log. It's append only and designed
//! to be a highly available source of truth for many different read models.
//!
//! The following implementation notes may be useful:
//! - Do the dumbest thing I can and test the hell out of it
//! - Allocate all memory at startup
//! - Direct I/O append only file, with KeyIndex that maps ID's to log offsets
//! - Storage engine Works at libc level (rustix), so you can follow man pages.
//! - Assumes linux, 64 bit, little endian - for now at least.
//! - Will orovide hooks to sync in the future, but actual HTTP (or whatever) server is out of scope.
#![cfg_attr(not(test), no_std)]
#[macro_use]
extern crate alloc;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");
#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");
// May work on other OSes but no testing has been done. Remove if you want!
#[cfg(not(target_os = "linux"))]
compile_error!("code assumes linux");

mod actor;
pub mod event;
pub mod fixcap;
pub mod mem;
mod pervasives;
pub mod storage;
#[cfg(test)]
mod test_utils;

pub use crate::pervasives::*;
pub use actor::*;
