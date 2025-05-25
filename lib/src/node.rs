extern crate alloc;
use core::{hash::Hash, mem};

use foldhash::fast::FixedState;
use hashbrown::{hash_map, HashMap};

// Kqueue's udata and io_uring's user_data are a void* and _u64 respectively
const _: () = assert!(mem::size_of::<*mut core::ffi::c_void>() == 8);

// Messages sent to an async file system with a req/res interface
mod fs {
    use super::{mem, topic, ID};

    #[repr(C)]
    struct CreateCtx {
        topic_id: topic::ID,
        node_id: ID,
        _padding: u32,
    }

    const _: () = assert!(mem::size_of::<CreateCtx>() == 8);

    pub fn create_udata(topic_id: topic::ID, node_id: ID) -> u64 {
        unsafe { mem::transmute(CreateCtx { topic_id, node_id, _padding: 0 }) }
    }

    pub enum Res<FD> {
        Create { fd: FD, udata: Create },
        //Read,
        //Append,
        //Delete,
    }
}

#[derive(Clone, Copy)]
struct ID(u8);

pub trait Path: rustix::path::Arg + Copy + Default + Eq + Hash {}

/// Top Level Response, for the user of the library
pub enum UsrRes<P: Path> {
    TopicCreated { name: P },
}

mod topic {
    use super::Path;
    use alloc::boxed::Box;

    pub struct ID(u8);
    const MAX: u8 = 64;

    #[derive(Debug)]
    pub enum CreateErr {
        DuplicateName,
        ReservationLimitExceeded,
    }

    /// Topics that are waiting to be created
    pub struct RequestedNames<Path> {
        /// Using an array so we can give each name a small "address"
        /// Ensure size is less than 256 bytes so we can use a u8 as the index
        names: Box<[Path; MAX as usize]>,
        /// Bitmask where 1 = occupied, 0 = available
        /// Allows us to remove names from the middle of the names array w/o
        /// re-ordering. If this array is empty, we've exceeded the capacity of
        /// names
        used_slots: u64,
    }

    impl<P: Path> RequestedNames<P> {
        pub fn new() -> Self {
            Self { names: Box::new([P::default(); 64]), used_slots: 0 }
        }

        pub fn add(&mut self, name: P) -> Result<ID, CreateErr> {
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

        pub fn remove(&mut self, topic_id: ID) -> P {
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
struct Core<P: Path, FD> {
    reqd_topic_names: topic::RequestedNames<P>,
    topic_fds: HashMap<P, FD, FixedState>, // Deterministic Hashmap
}

impl<P: Path, FD> Core<P, FD> {
    fn new(seed: u64) -> Self {
        Self {
            reqd_topic_names: topic::RequestedNames::new(),
            topic_fds: HashMap::with_hasher(FixedState::with_seed(seed)),
        }
    }

    fn create_topic(&mut self, name: P) -> Result<topic::ID, topic::CreateErr> {
        if self.topic_fds.contains_key(&name) {
            return Err(topic::CreateErr::DuplicateName);
        }

        let topic_id = self.reqd_topic_names.add(name)?;

        Ok(topic_id)
    }

    /// This turns internal DB and async io stuff into something relevant
    /// to the end user.
    /// It is one function, rather than one for each case, because I
    /// envison the result of this having a single callback associated with
    /// it in user code.
    /// TODO: review these assumptions
    fn fs_res_to_usr_res(&mut self, fs_res: fs::Res<FD>) -> UsrRes<P> {
        match fs_res {
            fs::Res::Create { fd, udata } => {
                let name = self.reqd_topic_names.remove(udata.topic_id);
                match self.topic_fds.entry(name) {
                    hash_map::Entry::Vacant(entry) => {
                        entry.insert(fd);
                    }
                    hash_map::Entry::Occupied(_) => {
                        panic!("failed to reserve topic name")
                    }
                }
                UsrRes::TopicCreated { name }
            }
        }
    }
}

// Non-deterministic part.
// Wraps io_uring, kqueue, testing etc
pub trait AsyncFSIO {
    type P: Path;
    type FD;

    fn new(root_dir: Self::P) -> Self;

    fn create(&mut self, path: Self::P, udata: u64);
    fn read(&mut self, fd: Self::FD, udata: u64);
    fn append(&mut self, fd: Self::FD, udata: u64);
    fn delete(&mut self, fd: Self::FD, udata: u64);
}

pub struct Node<AFIO: AsyncFSIO> {
    id: ID,
    afio: AFIO,
    core: Core<AFIO::P, AFIO::FD>,
}

impl<AFIO: AsyncFSIO> Node<AFIO> {
    fn new(seed: u64, id: ID, root_dir: AFIO::P) -> Self {
        let afio = AFIO::new(root_dir);
        let core = Core::new(seed);
        Self { id, afio, core }
    }

    pub fn topic_create(
        &mut self,
        name: AFIO::P,
    ) -> Result<(), topic::CreateErr> {
        let topic_id = self.core.create_topic(name)?;
        let udata = fs::create_udata(topic_id, self.id);
        self.afio.create(name, udata);
        Ok(())
    }

    pub fn local_events_append(&mut self) {}
}
