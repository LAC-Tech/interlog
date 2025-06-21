extern crate alloc;
use core::mem;

use portable_atomic::AtomicBool;

//use crate::deterministic_hash_map::{Entry, Ext, HashMap};
use crate::slotmap::SlotMap;

mod async_io {
    use core::{convert, fmt};
    pub struct Res<FD> {
        pub rc: FD,
        pub usr_data: u64,
    }

    pub trait ReqFactory {
        type FD: Copy
            + Default
            + Eq
            // So I can unwrap
            + convert::TryInto<usize, Error: core::fmt::Debug>;
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
    use core::ops::Deref;
    use core::sync::atomic::AtomicU32;
    use core::{
        ffi,
        mem::{align_of, size_of},
        ops::{Index, IndexMut},
        ptr,
    };
    use rustix::fd::AsFd;
    use rustix::io_uring::{
        addr_or_splice_off_in_union, io_uring_cqe, io_uring_params,
        io_uring_setup, io_uring_sqe, io_uring_user_data, ioprio_union,
        len_union, IoringAcceptFlags, IoringFeatureFlags, IoringOp,
        IoringSqeFlags, IORING_OFF_SQES, IORING_OFF_SQ_RING,
    };
    use rustix::mm;
    use rustix::mm::mmap;
    use rustix::net::{bind, listen, socket, sockopt, Ipv4Addr, SocketAddrV4};
    use rustix::{fd, io, net};

    struct AsyncIO {
        socket_fd: fd::OwnedFd,
    }

    impl AsyncIO {
        fn new() -> io::Result<Self> {
            let socket_fd = socket(
                net::AddressFamily::INET,
                net::SocketType::STREAM,
                Some(net::ipproto::TCP),
            )?;

            sockopt::set_socket_reuseaddr(socket_fd.as_fd(), true)?;

            let port = 12345;
            let addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port);
            bind(socket_fd.as_fd(), &addr)?;
            listen(socket_fd.as_fd(), 128)?;
            Ok(Self { socket_fd })
        }
    }

    struct Ring<'a> {
        fd: fd::OwnedFd,
        ring_buf: Mmap<u8>,
        sq: SubmissionQueue,
        cq: CompletionQueue<'a>,
    }

    impl<'a> Ring<'a> {
        fn with_params(
            entries: u32,
            p: &mut io_uring_params,
        ) -> Result<Self, RingErr> {
            use io::Errno;
            use RingErr::*;

            if entries == 0 {
                return Err(EntriesZero);
            }
            if !entries.is_power_of_two() {
                return Err(EntriesNotPowerOfTwo);
            }

            let res = unsafe { io_uring_setup(entries, p) };

            let fd = res.map_err(|errno| match errno {
                Errno::FAULT => ParamsOutsideAccessibleAddressSpace,
                // The resv array contains non-zero data, p.flags contains an
                // unsupported flag, entries out of bounds, IORING_SETUP_SQ_AFF
                // was specified without IORING_SETUP_SQPOLL, or
                // IORING_SETUP_CQSIZE was specified but
                // linux.io_uring_params.cq_entries was invalid:
                Errno::INVAL => ArgumentsInvalid,
                Errno::MFILE => ProcessFdQuotaExceeded,
                Errno::NFILE => SystemFdQuotaExceeded,
                Errno::NOMEM => SystemResources,
                // IORING_SETUP_SQPOLL was specified but effective user ID lacks
                // sufficient privileges, or a container seccomp policy
                // prohibits io_uring syscalls:
                Errno::PERM => PermissionDenied,
                Errno::NOSYS => SystemOutdated,
                _ => Unexpected(errno),
            })?;

            // Kernel versions 5.4 and up use only one mmap() for the submission
            // and completion queues. This is not an optional feature for us...
            // if the kernel does it, we have to do it.
            // The thinking on this by the kernel developers was that both the
            // submission and the completion queue rings have sizes just over a
            // power of two, but the submission queue ring is significantly
            // smaller with u32 slots. By bundling both in a single mmap, the
            // kernel gets the submission queue ring for free.
            // See https://patchwork.kernel.org/patch/11115257 for the kernel
            // patch.
            // We do not support the double mmap() done before 5.4, because we
            // want to keep the init/deinit mmap paths simple and because
            // io_uring has had many bug fixes even since 5.4.
            if !p.features.contains(IoringFeatureFlags::SINGLE_MMAP) {
                return Err(SystemOutdated);
            }

            // Check that the kernel has actually set params and that
            // "impossible is nothing".
            assert_ne!(p.sq_entries, 0);
            assert_ne!(p.cq_entries, 0);
            assert!(p.cq_entries >= p.sq_entries);

            let size = core::cmp::max(
                // This one is in the man page for io_uring_enter
                p.sq_off.array + p.sq_entries * size_of::<u32>() as u32,
                // WTF is this one?!
                p.cq_off.cqes + p.cq_entries * size_of::<io_uring_cqe>() as u32,
            ) as usize;

            let ring_buf = unsafe {
                Mmap::<u8>::new(size, fd.as_fd(), IORING_OFF_SQ_RING)
                    .map_err(Unexpected)
            }?;
            // From here on, we only need to read from params, so pass `p` by
            // value as immutable.
            // The completion queue shares the mmap with the submission queue,
            // so pass `sq` there too.

            let sq = SubmissionQueue::new(fd.as_fd(), p, &ring_buf)
                .map_err(Unexpected)?;
            let cq = CompletionQueue::new(p, &ring_buf).map_err(Unexpected)?;

            Ok(Self { fd, ring_buf, sq, cq })
        }
    }

    enum RingErr {
        EntriesZero,
        EntriesNotPowerOfTwo,
        ParamsOutsideAccessibleAddressSpace,
        ArgumentsInvalid,
        ProcessFdQuotaExceeded,
        SystemFdQuotaExceeded,
        SystemResources,
        PermissionDenied,
        SystemOutdated,
        Unexpected(io::Errno),
    }

    struct SubmissionQueue {
        head: *const AtomicU32,
        tail: *const AtomicU32,
        mask: u32,
        flags: *const AtomicU32,
        dropped: *const AtomicU32,
        sqes: Mmap<io_uring_sqe>,
        // We use `sqe_head` and `sqe_tail` in the same way as liburing:
        // We increment `sqe_tail` (but not `tail`) for each call to
        // `get_sqe()`.
        // We then set `tail` to `sqe_tail` once, only when these events are
        // actually submitted.
        // This allows us to amortize the cost of the @atomicStore to `tail`
        // across multiple SQEs.
        sqe_head: u32,
        sqe_tail: u32,
    }

    impl SubmissionQueue {
        fn new(
            fd: fd::BorrowedFd,
            p: &io_uring_params,
            ring_buf: &Mmap<u8>,
        ) -> io::Result<Self> {
            let size_sqes = p.sq_entries as usize * size_of::<io_uring_sqe>();
            let sqes = unsafe {
                Mmap::<io_uring_sqe>::new(size_sqes, fd, IORING_OFF_SQES)
            }?;

            let result = unsafe {
                Self {
                    head: *(ring_buf.ptr.add(p.sq_off.head as usize))
                        as *const AtomicU32,
                    tail: *(ring_buf.ptr.add(p.sq_off.tail as usize))
                        as *const AtomicU32,
                    mask: *(ring_buf.ptr.add(p.sq_off.ring_mask as usize)
                        as *const u32),
                    flags: *(ring_buf.ptr.add(p.sq_off.flags as usize))
                        as *const AtomicU32,
                    dropped: *(ring_buf.ptr.add(p.sq_off.dropped as usize))
                        as *const AtomicU32,
                    sqes,
                    sqe_head: 0,
                    sqe_tail: 0,
                }
            };

            Ok(result)
        }
    }

    struct CompletionQueue<'a> {
        head: *const AtomicU32,
        tail: *const AtomicU32,
        mask: u32,
        overflow: *const AtomicU32,
        cqes: &'a [io_uring_cqe],
    }

    impl<'a> CompletionQueue<'a> {
        fn new(p: &io_uring_params, ring_buf: &Mmap<u8>) -> io::Result<Self> {
            let ring_entries = unsafe {
                *(ring_buf.ptr.add(p.cq_off.ring_entries as usize)
                    as *const u32)
            };
            if p.cq_entries != ring_entries {
                return Err(io::Errno::INVAL);
            }
            let cqes = unsafe {
                let ptr = ring_buf.ptr.add(p.cq_off.cqes as usize)
                    as *const io_uring_cqe;
                std::slice::from_raw_parts(ptr, p.cq_entries as usize)
            };
            Ok(unsafe {
                Self {
                    head: ring_buf.ptr.add(p.cq_off.head as usize)
                        as *const AtomicU32,
                    tail: ring_buf.ptr.add(p.cq_off.tail as usize)
                        as *const AtomicU32,
                    mask: *(ring_buf.ptr.add(p.cq_off.ring_mask as usize)
                        as *const u32),
                    overflow: ring_buf.ptr.add(p.cq_off.overflow as usize)
                        as *const AtomicU32,
                    cqes,
                }
            })
        }
    }

    struct Mmap<T: Sized> {
        ptr: *mut T,
        n_elems: usize,
    }

    impl<T: Sized> Mmap<T> {
        pub unsafe fn new<FD>(
            size: usize,
            fd: FD,
            offset: u64,
        ) -> io::Result<Self>
        where
            FD: fd::AsFd,
        {
            let elem_size = size_of::<T>();
            assert_eq!(size % elem_size, 0);
            assert_eq!(offset % elem_size as u64, 0);

            let ptr = unsafe {
                mmap(
                    ptr::null_mut(),
                    size,
                    mm::ProtFlags::READ | mm::ProtFlags::WRITE,
                    mm::MapFlags::SHARED | mm::MapFlags::POPULATE,
                    fd,
                    offset,
                )?
            };
            assert!(!ptr.is_null());
            assert_eq!(ptr as usize % align_of::<T>(), 0);

            let n_elems = size / elem_size;

            Ok(Self { ptr: ptr as *mut T, n_elems })
        }
    }

    impl<T: Sized> Drop for Mmap<T> {
        fn drop(&mut self) {
            unsafe {
                let size = self.n_elems * size_of::<T>();
                mm::munmap(self.ptr as *mut ffi::c_void, size).unwrap();
            }
        }
    }

    impl<T: Sized> Deref for Mmap<T> {
        type Target = [T];

        fn deref(&self) -> &[T] {
            unsafe { core::slice::from_raw_parts(self.ptr, self.n_elems) }
        }
    }

    impl<T: Sized> AsRef<T> for Mmap<T>
    where
        <Mmap<T> as Deref>::Target: AsRef<T>,
    {
        fn as_ref(&self) -> &T {
            self.deref().as_ref()
        }
    }

    struct ReqFactory;

    impl async_io::ReqFactory for ReqFactory {
        // Not reasoning about FD lifetimes at this level
        type FD = fd::RawFd;
        type Req = io_uring_sqe;

        fn accept_multishot(usr_data: u64, fd: fd::RawFd) -> Self::Req {
            Self::Req {
                opcode: IoringOp::Accept,
                flags: IoringSqeFlags::empty(),
                ioprio: ioprio_union {
                    accept_flags: IoringAcceptFlags::MULTISHOT,
                },
                fd,
                user_data: io_uring_user_data { u64_: usr_data },
                ..Default::default()
            }
        }

        fn recv(usr_data: u64, fd: Self::FD, buf: &mut [u8]) -> Self::Req {
            Self::Req {
                opcode: IoringOp::Recv,
                flags: IoringSqeFlags::empty(),
                fd,
                user_data: io_uring_user_data { u64_: usr_data },
                addr_or_splice_off_in: addr_or_splice_off_in_union {
                    addr: (buf.as_mut_ptr() as *mut ffi::c_void).into(),
                },
                len: len_union { len: buf.len() as u32 },
                ..Default::default()
            }
        }

        fn send(usr_data: u64, fd: Self::FD, buf: &[u8]) -> Self::Req {
            Self::Req {
                opcode: IoringOp::Send,
                flags: IoringSqeFlags::empty(),
                fd,
                user_data: io_uring_user_data { u64_: usr_data },
                addr_or_splice_off_in: addr_or_splice_off_in_union {
                    addr: (buf.as_ptr() as *mut ffi::c_void).into(),
                },
                len: len_union { len: buf.len() as u32 },
                ..Default::default()
            }
        }
    }
}

mod in_mem {
    use super::async_io;
    use crate::no_alloc_vec;
    use crate::slotmap;
    use crate::slotmap::SlotMap;
    use core::{ffi, marker, mem};

    struct InMem<'a, RF: async_io::ReqFactory> {
        clients: SlotMap<'a, RF::FD, MAX_CLIENTS>,
        recv_buf: &'a mut [u8],
        aio_req_buf: no_alloc_vec::Stack<RF::Req, 2>,
        _rf: marker::PhantomData<RF>,
    }

    fn initial_aio_req() -> u64 {
        UsrData::Accept.as_u64()
    }

    impl<'a, RF: async_io::ReqFactory> InMem<'a, RF> {
        fn new(
            recv_buf: &'a mut [u8],
            client_fds_buf: &'a mut [RF::FD; MAX_CLIENTS],
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
            res: async_io::Res<RF::FD>,
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
