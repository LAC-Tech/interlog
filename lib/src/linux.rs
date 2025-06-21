use core::ops::Deref;
use core::sync::atomic::AtomicU32;
use core::{
    ffi,
    mem::{align_of, size_of},
    ptr,
};
use rustix::fd::AsFd;
use rustix::io_uring::{
    addr_or_splice_off_in_union, io_uring_cqe, io_uring_params, io_uring_setup,
    io_uring_sqe, io_uring_user_data, ioprio_union, len_union,
    IoringAcceptFlags, IoringFeatureFlags, IoringOp, IoringSqeFlags,
    IORING_OFF_SQES, IORING_OFF_SQ_RING,
};
use rustix::mm;
use rustix::mm::mmap;
use rustix::net::{bind, listen, socket, sockopt, Ipv4Addr, SocketAddrV4};
use rustix::{fd, io, net};

use crate::aio;

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
            *(ring_buf.ptr.add(p.cq_off.ring_entries as usize) as *const u32)
        };
        if p.cq_entries != ring_entries {
            return Err(io::Errno::INVAL);
        }
        let cqes = unsafe {
            let ptr =
                ring_buf.ptr.add(p.cq_off.cqes as usize) as *const io_uring_cqe;
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
    pub unsafe fn new<FD>(size: usize, fd: FD, offset: u64) -> io::Result<Self>
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

impl aio::ReqFactory for ReqFactory {
    // Not reasoning about FD lifetimes at this level
    type FD = fd::RawFd;
    type Req = io_uring_sqe;

    fn accept_multishot(usr_data: u64, fd: fd::RawFd) -> Self::Req {
        Self::Req {
            opcode: IoringOp::Accept,
            flags: IoringSqeFlags::empty(),
            ioprio: ioprio_union { accept_flags: IoringAcceptFlags::MULTISHOT },
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
