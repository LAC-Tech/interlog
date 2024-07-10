//! All publicly facing erros for interlog.
//!
//! By definition, all these errors are unrecoverable. Some are bugs, some are
//! not (ie, overrunning memory limits that a user configured).
//!
//! Reasons I am not using panic:
//! - I want to be able to catch these, and rust's unwind has too many caveats
//! - I want descriptive errors without allocating big fancy format strings
//!
//! I've also opted to make them rich, rather than flat C style errors. I value
//! the correct context more than an ergonomic C API.

use crate::mem;

#[derive(Debug)]
pub enum Enqueue {
	Log(mem::Overrun),
	Index(mem::Overrun),
}

impl Enqueue {
	pub fn is_bug(self) -> bool {
		panic!("implement")
	}
}

#[derive(Debug)]
pub enum Commit<StorageWriteErr> {
	Index(mem::Overrun),
	Log(StorageWriteErr),
}

impl<StorageWriteErr> Commit<StorageWriteErr> {
	pub fn is_bug(self) -> bool {
		panic!("implement")
	}
}
