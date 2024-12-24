//! The core of interog.
//!
//! Each log is
//! - single threaded
//! - synchronous
//! - persists to a single append-only fil
//!
//! Core has all the primitives needed to implement sync, but does not call out
//! to the network itself.
//!
//! The following implementation notes may be useful:
//! - Do the dumbest thing I can and test the hell out of it
//! - Allocate all memory at startup
//! - Direct I/O append only file, with KeyIndex that maps ID's to log offsets
//! - Storage engine Works at libc level (rustix), so you can follow man pages.
//! - Assumes linux, 64 bit, little endian - for now at least.
//! - Will orovide hooks to sync in the future, but actual HTTP (or whatever) server is out of scope.

// This lets me add dbg! statements using "#[cfg(test)]"
#![cfg_attr(not(test), no_std)]

#[macro_use]
extern crate alloc;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("code assumes usize is u64");
#[cfg(not(target_endian = "little"))]
compile_error!("code assumes little-endian");

mod linux;
pub mod log;
mod node;
