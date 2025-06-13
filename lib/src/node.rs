extern crate alloc;
use core::mem;

use portable_atomic::AtomicBool;

//use crate::deterministic_hash_map::{Entry, Ext, HashMap};
use crate::slotmap::SlotMap;

mod async_io {
    pub struct Res<FD> {
        pub rc: FD,
        pub usr_data: u64,
    }

    pub trait ReqFactory {
        type FD;
        type Req: Copy + Default;
        fn accept_multishot(usr_data: u64, fd: Self::FD) -> Self::Req;
        fn recv(usr_data: u64, fd: Self::FD, buf: &mut [u8]) -> Self::Req;
        fn send(usr_data: u64, fd: Self::FD, buf: &[u8]) -> Self::Req;
    }

    pub trait AsyncIO<RF: ReqFactory> {
        /// Non blocking
        fn submit(&self, reqs: &[RF::Req]) -> usize;

        /// Blocking
        fn wait_for_res(&self) -> Res<RF::FD>;
    }
}

mod linux {
    use super::async_io;
    use rustix::fd::AsFd;
    use rustix::io_uring::io_uring_sqe;

    struct ReqFactory;

    impl async_io::ReqFactory for ReqFactory {
        // TODO: I have no idea about how to reason about FD lifetimes
    }
}

mod in_mem {
    use super::async_io;
    use crate::no_alloc_vec;
    use crate::slotmap;
    use crate::slotmap::SlotMap;
    use core::{convert, ffi, fmt, marker, mem};

    struct InMem<'a, FD, RF: async_io::ReqFactory<FD>> {
        clients: SlotMap<'a, FD, MAX_CLIENTS>,
        recv_buf: &'a mut [u8],
        aio_req_buf: no_alloc_vec::Stack<RF::Req, 2>,
        _rf: marker::PhantomData<RF>,
    }

    fn initial_aio_req() -> u64 {
        UsrData::Accept.as_u64()
    }

    impl<'a, FD, RF> InMem<'a, FD, RF>
    where
        FD: Copy + Default + Eq + convert::TryInto<usize>,
        <FD as convert::TryInto<usize>>::Error: fmt::Debug,
        RF: async_io::ReqFactory<FD>,
    {
        fn new(
            recv_buf: &'a mut [u8],
            client_fds_buf: &'a mut [FD; MAX_CLIENTS],
        ) -> Self {
            let client_fds = SlotMap::new(client_fds_buf);
            let aio_req_buf = no_alloc_vec::create_on_stack();
            let _rf = marker::PhantomData;
            Self { clients: client_fds, recv_buf, aio_req_buf, _rf }
        }

        fn prepare_client(&mut self, client_id: u8) -> RF::Req {
            let fd = self.clients.get(client_id).unwrap();
            let usr_data = UsrData::Recv { client_id }.as_u64();
            RF::recv(usr_data, fd, self.recv_buf)
        }

        fn handle_aio_res<'b>(
            &'b mut self,
            res: async_io::Res<FD>,
        ) -> Result<&'b [RF::Req], Err> {
            self.aio_req_buf.clear();

            let usr_data = UsrData::from_u64(res.usr_data);
            match usr_data {
                UsrData::Accept => {
                    let fd_client = res.rc;
                    let client_id =
                        self.clients.add(fd_client).map_err(Err::Client)?;

                    let req = RF::send(
                        UsrData::Send { client_id }.as_u64(),
                        fd_client,
                        b"connection acknowledged\n",
                    );

                    self.aio_req_buf.push(req).map_err(Err::AIOReq)?;
                }
                UsrData::Send { client_id } => {
                    let req = self.prepare_client(client_id);
                    self.aio_req_buf.push(req).map_err(Err::AIOReq)?;
                }
                UsrData::Recv { client_id } => {
                    let buf_len: usize = res.rc.try_into().unwrap();
                    #[cfg(debug_assertions)]
                    dbg!(&self.recv_buf[0..buf_len]);

                    let req = self.prepare_client(client_id);
                    self.aio_req_buf.push(req).map_err(Err::AIOReq)?;
                }
            }

            Ok(self.aio_req_buf.as_slice())
        }
    }

    pub enum Err {
        Client(slotmap::Err),
        AIOReq(no_alloc_vec::Err),
    }

    const MAX_CLIENTS: usize = 2;

    #[repr(align(8))]
    #[cfg_attr(
        test,
        derive(
            arbtest::arbitrary::Arbitrary,
            Clone,
            Copy,
            Debug,
            PartialEq,
            Eq
        )
    )]
    enum UsrData {
        Accept,
        Recv { client_id: u8 },
        Send { client_id: u8 },
    }

    impl UsrData {
        fn as_u64(self) -> u64 {
            unsafe { mem::transmute(self) }
        }

        fn from_u64(u: u64) -> Self {
            unsafe { mem::transmute(u) }
        }
    }

    // Kqueue's udata and io_uring's user_data are void* and _u64 respectively
    const _: () = assert!(8 == mem::size_of::<*mut ffi::c_void>());
    const _: () = assert!(8 == mem::size_of::<UsrData>());

    #[cfg(test)]
    mod tests {
        use super::*;
        use arbtest::arbtest;
        use pretty_assertions::assert_eq;

        // There we go now my transmuting is safe
        #[test]
        fn usr_data_casting() {
            arbtest(|u| {
                let expected: UsrData = u.arbitrary()?;
                let actual = UsrData::from_u64(expected.as_u64());
                assert_eq!(actual, expected);

                Ok(())
            });
        }
    }
}

/*
/* DATA **********************************************************************/

pub struct Node<AFS: fs::AsyncIO> {
    id: ID,
    afs: AFS,
    core: Core<AFS::P, AFS::FD>,
    running: AtomicBool,
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
    /// Topics that are waiting to be created
    /// This is to prevent two requests trying to reserve the same name
    // TODO: this seems like it'd barely ever happen.
    // Wouldn't the FS fail because the name was the same?
    reqd_topic_names: SlotMap<P>,
    topics: HashMap<P, Topic<FD>>,
}

struct Topic<FD> {
    local: AppendOnlyFile<FD>,
    replicas: HashMap<ID, AppendOnlyFile<FD>>,
}

pub struct TopicID(u8);

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
        let running = AtomicBool::new(true);
        Self { id, afs, core, running }
    }

    pub fn topic_create(&mut self, name: AFS::P) -> Result<(), slotmap::Err> {
        let topic_id = self.core.create_topic(name)?;
        let udata = fs::create_udata(topic_id, self.id);
        self.afs.create(name, udata);
        Ok(())
    }

    pub fn local_events_append(&mut self) {
        panic!("TODO")
    }

    pub fn start_event_loop(&mut self, handler: impl Fn(UsrRes<AFS::P>)) {
        while self.running.load(core::sync::atomic::Ordering::SeqCst) {
            let fs_res = self.afs.wait_for_res();
            let usr_res = self.core.fs_res_to_usr_res(fs_res);
            handler(usr_res)
        }
    }

    pub fn quit_event_loop(&mut self) {
        self.running.swap(false, core::sync::atomic::Ordering::SeqCst);
    }
}

impl<P: fs::Path, FD> Core<P, FD> {
    fn new(seed: u64) -> Self {
        let reqd_topic_names = SlotMap::new();
        let topic_aofs = HashMap::new(seed);
        Self { seed, reqd_topic_names, topics: topic_aofs }
    }

    fn create_topic(&mut self, name: P) -> Result<TopicID, slotmap::Err> {
        let slot = self.reqd_topic_names.add(name)?;
        Ok(TopicID(slot))
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
                let name = self.reqd_topic_names.remove(udata.topic_id.0);
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
    use super::{mem, TopicID, ID};
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
        pub topic_id: TopicID,
        node_id: ID,
        _padding: u32,
    }

    pub fn create_udata(topic_id: TopicID, node_id: ID) -> u64 {
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
*/
