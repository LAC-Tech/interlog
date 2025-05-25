extern crate alloc;
use core::mem;

use foldhash::fast::FixedState;
use hashbrown::{hash_map, HashMap};

// Kqueue's udata and io_uring's user_data are a void* and _u64 respectively
const _: () = assert!(mem::size_of::<*mut core::ffi::c_void>() == 8);

// Messages sent to an async file system with a req/res interface
mod fs {
	use super::topic;

	pub enum Req<FD> {
		Create { topic_id: topic::ID },
		//Read { fd: FD },
		//Append { fd: FD },
		Delete { fd: FD },
	}

	pub enum Res<FD> {
		Create { fd: FD, topic_id: topic::ID },
		//Read,
		//Append,
		//Delete,
	}
}

/// Top Level Response, for the user of the library
mod usr {
	pub enum Req<'a> {
		CreateTopic { name: &'a [u8] },
	}

	pub enum Res<'a> {
		TopicCreated { name: &'a [u8] },
	}
}

mod topic {
	use alloc::boxed::Box;

	pub struct ID(u8);
	const MAX: u8 = 64;

	#[derive(Debug)]
	pub enum CreateErr {
		DuplicateName,
		ReservationLimitExceeded,
	}

	/// Topics that are waiting to be created
	pub struct RequestedNames<'a> {
		/// Using an array so we can give each name a small "address"
		/// Ensure size is less than 256 bytes so we can use a u8 as the index
		names: Box<[&'a [u8]; MAX as usize]>,
		/// Bitmask where 1 = occupied, 0 = available
		/// Allows us to remove names from the middle of the names array w/o
		/// re-ordering. If this array is empty, we've exceeded the capacity of
		/// names
		used_slots: u64,
	}

	impl<'a> RequestedNames<'a> {
		pub fn new() -> Self {
			Self { names: Box::new([b""; 64]), used_slots: 0 }
		}

		pub fn add(&mut self, name: &'a [u8]) -> Result<ID, CreateErr> {
			if self.names.contains(&name) {
				return Err(CreateErr::DuplicateName);
			}

			// Find first free slot
			let idx = (!self.used_slots).trailing_zeros() as u8;
			if idx >= MAX {
				return Err(CreateErr::ReservationLimitExceeded);
			}

			// Mark slot as used
			self.used_slots |= 1u64 << idx;
			self.names[idx as usize] = name;

			Ok(ID(idx))
		}

		pub fn remove(&mut self, topic_id: ID) -> &'a [u8] {
			assert!(topic_id.0 < MAX, "Index out of bounds");
			self.used_slots &= !(1u64 << topic_id.0);
			core::mem::take(&mut self.names[topic_id.0 as usize])
		}
	}
}

/// This structs job is to receive "completed" async fs events, and:
/// 1 - update reflect changs to the node in memory and
/// 2 - return a meaningful response for user code
/// It's completey decoupled from any async runtime
pub struct Node<'a, FD> {
	reqd_topic_names: topic::RequestedNames<'a>,
	topic_fds: HashMap<&'a [u8], FD, FixedState>, // Deterministic Hashmap
}

impl<'a, FD> Node<'a, FD> {
	pub fn new(seed: u64) -> Self {
		Self {
			reqd_topic_names: topic::RequestedNames::new(),
			topic_fds: HashMap::with_hasher(FixedState::with_seed(seed)),
		}
	}

	/// The topic name is raw bytes; focusing on linux first, and ext4
	/// filenames are bytes, not a particular encoding.
	/// TODO: some way of translating this into the the platforms native
	/// filename format ie utf-8 for OS X, utf-16 for windows
	pub fn req_usr_to_fs(
		&mut self,
		usr_req: usr::Req<'a>,
	) -> Result<fs::Req<FD>, topic::CreateErr> {
		match usr_req {
			usr::Req::CreateTopic { name } => {
				if self.topic_fds.contains_key(name) {
					return Err(topic::CreateErr::DuplicateName);
				}

				let topic_id = self.reqd_topic_names.add(name)?;

				Ok(fs::Req::Create { topic_id })
			}
		}
	}

	/// This turns internal DB and async io stuff into something relevant
	/// to the end user.
	/// It is one function, rather than one for each case, because I
	/// envison the result of this having a single callback associated with
	/// it in user code.
	/// TODO: review these assumptions
	pub fn res_fs_to_usr(&mut self, fs_res: fs::Res<FD>) -> usr::Res {
		match fs_res {
			fs::Res::Create { fd, topic_id } => {
				let name = self.reqd_topic_names.remove(topic_id);
				match self.topic_fds.entry(name) {
					hash_map::Entry::Vacant(entry) => {
						entry.insert(fd);
					}
					hash_map::Entry::Occupied(_) => {
						panic!("failed to reserve topic name")
					}
				}
				usr::Res::TopicCreated { name }
			}
		}
	}
}
