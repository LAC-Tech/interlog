use rustix::{fd, fs, io, mm, path};

pub struct MmapStorage {
	mmap_ptr: *const u8,
	mmap_size: usize,
	fd: fd::OwnedFd,
	n_bytes_appended: usize,
}

impl MmapStorage {
	pub fn new<P: path::Arg>(path: P, mmap_size: usize) -> io::Result<Self> {
		let fd = fs::open(
			path,
			fs::OFlags::CREATE | fs::OFlags::APPEND,
			fs::Mode::RUSR | fs::Mode::WUSR,
		)?;

		// Flush any uncommitted writes
		fs::fsync(&fd)?;

		let mmap_ptr = unsafe {
			mm::mmap(
				core::ptr::null_mut(),
				mmap_size,
				mm::ProtFlags::READ,
				mm::MapFlags::SHARED,
				&fd,
				0,
			)
		}?;

		let mmap_ptr = mmap_ptr as *const u8;

		// TODO: need a separate checkpoint file that tells us this.
		let n_bytes_appended = 0;
		Ok(Self { mmap_ptr, mmap_size, fd, n_bytes_appended })
	}
}

impl ports::Storage for MmapStorage {
	type Err = io::Errno;
	fn append(&mut self, data: &[u8]) -> io::Result<()> {
		if self.n_bytes_appended + data.len() > self.mmap_size {
			panic!("Not enough space in mmap for append");
		}

		io::write(&self.fd, data)?;
		fs::fsync(&self.fd)?;
		self.n_bytes_appended += data.len();
		Ok(())
	}

	fn read(&self) -> &[u8] {
		unsafe {
			core::slice::from_raw_parts(self.mmap_ptr, self.n_bytes_appended)
		}
	}

	fn size(&self) -> usize {
		self.n_bytes_appended
	}
}

impl Drop for MmapStorage {
	fn drop(&mut self) {
		unsafe {
			mm::munmap(self.mmap_ptr as *mut _, self.mmap_size).unwrap();
		}
	}
}
