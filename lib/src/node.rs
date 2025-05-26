extern crate alloc;
use core::mem;

use crate::deterministic_hash_map::{Entry, Ext, HashMap};

// Kqueue's udata and io_uring's user_data are a void* and _u64 respectively
const _: () = assert!(8 == mem::size_of::<*mut core::ffi::c_void>());

/* DATA **********************************************************************/

pub struct Node<AFS: fs::AsyncIO> {
    id: ID,
    afs: AFS,
    core: Core<AFS::P, AFS::FD>,
}

#[derive(Clone, Copy)]
struct ID(u8);

/// This structs job is to receive "completed" async fs events, and:
/// 1 - update reflect changs to the node in memory and
/// 2 - return a meaningful response for user code
/// It's completey decoupled from any async runtime
struct Core<P: fs::Path, FD> {
    /// Used for making various hashmaps deterministic
    seed: u64,
    reqd_topic_names: topic::RequestedNames<P>,
    topics: HashMap<P, Topic<FD>>,
}

struct Topic<FD> {
    local: AppendOnlyFile<FD>,
    replicas: HashMap<ID, AppendOnlyFile<FD>>,
}

struct AppendOnlyFile<FD> {
    fd: FD,
}

/// Top Level Response, for the user of the library
pub enum UsrRes<P: fs::Path> {
    TopicCreated { name: P },
}

/* IMPL **********************************************************************/

impl<AFS: fs::AsyncIO> Node<AFS> {
    fn new(seed: u64, id: ID, root_dir: AFS::P) -> Self {
        let afs = AFS::new(root_dir);
        let core = Core::new(seed);
        Self { id, afs, core }
    }

    pub fn topic_create(
        &mut self,
        name: AFS::P,
    ) -> Result<(), topic::CreateErr> {
        let topic_id = self.core.create_topic(name)?;
        let udata = fs::create_udata(topic_id, self.id);
        self.afs.create(name, udata);
        Ok(())
    }

    pub fn local_events_append(&mut self) {
        panic!("TODO")
    }

    pub fn run_event_loop(&mut self, handler: impl Fn(UsrRes<AFS::P>)) {
        loop {
            let fs_res = self.afs.wait_for_res();
            let usr_res = self.core.fs_res_to_usr_res(fs_res);
            handler(usr_res)
        }
    }
}

impl<P: fs::Path, FD> Core<P, FD> {
    fn new(seed: u64) -> Self {
        let reqd_topic_names = topic::RequestedNames::new();
        let topic_aofs = HashMap::new(seed);
        Self { seed, reqd_topic_names, topics: topic_aofs }
    }

    fn create_topic(&mut self, name: P) -> Result<topic::ID, topic::CreateErr> {
        if self.topics.contains_key(&name) {
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
                match self.topics.entry(name) {
                    Entry::Vacant(entry) => {
                        let aofs = Topic::new(fd, self.seed);
                        entry.insert(aofs);
                    }
                    Entry::Occupied(_) => {
                        panic!("failed to reserve topic name")
                    }
                }
                UsrRes::TopicCreated { name }
            }
        }
    }
}

impl<FD> Topic<FD> {
    fn new(local_fd: FD, seed: u64) -> Self {
        Self {
            local: AppendOnlyFile::new(local_fd),
            replicas: HashMap::new(seed),
        }
    }
}

impl<FD> AppendOnlyFile<FD> {
    fn new(fd: FD) -> Self {
        Self { fd }
    }
}

// Messages sent to an async file system with a req/res interface
mod fs {
    use super::{mem, topic, ID};
    use core::hash::Hash;

    const _: () = assert!(8 == mem::size_of::<CreateCtx>());

    pub enum Res<FD> {
        Create { fd: FD, udata: CreateCtx },
        //Read,
        //Append,
        //Delete,
    }

    #[repr(C)]
    pub struct CreateCtx {
        pub topic_id: topic::ID,
        node_id: ID,
        _padding: u32,
    }

    pub fn create_udata(topic_id: topic::ID, node_id: ID) -> u64 {
        unsafe { mem::transmute(CreateCtx { topic_id, node_id, _padding: 0 }) }
    }

    impl CreateCtx {
        pub fn from_u64(udata: u64) -> Self {
            unsafe { mem::transmute(udata) }
        }
    }

    pub trait Path: rustix::path::Arg + Copy + Default + Eq + Hash {}

    // Non-deterministic part.
    // Wraps io_uring, kqueue, testing etc
    pub trait AsyncIO {
        type P: Path;
        type FD;

        fn new(root_dir: Self::P) -> Self;

        fn create(&mut self, path: Self::P, udata: u64);
        fn read(&mut self, fd: Self::FD, udata: u64);
        fn append(&mut self, fd: Self::FD, udata: u64);
        fn delete(&mut self, fd: Self::FD, udata: u64);

        fn wait_for_res(&self) -> Res<Self::FD>;
    }
}

mod topic {
    use super::{fs, mem};
    use alloc::boxed::Box;

    pub struct ID(u8);
    const MAX: u8 = 64;

    #[derive(Debug)]
    pub enum CreateErr {
        DuplicateName,
        ReservationLimitExceeded,
    }

    /// Topics that are waiting to be created
    /// This is to prevent two requests trying to reserve the same name
    // TODO: this seems like it'd barely ever happen.
    // Wouldn't the FS fail because the name was the same?
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

    impl<P: fs::Path> RequestedNames<P> {
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
            let name = &mut self.names[topic_id.0 as usize];
            mem::take(name)
        }
    }
}
