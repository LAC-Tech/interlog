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

		let stat = fs::fstat(&fd)?;
		let n_bytes_appended: usize = stat.st_size.try_into().unwrap();

		Ok(Self { mmap_ptr, mmap_size, fd, n_bytes_appended })
	}
}

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub enum Syscall {
	Write,
	Fsync,
}

impl ports::Storage for MmapStorage {
	type Err = (Syscall, io::Errno);
	fn append(&mut self, data: &[u8]) -> Result<(), Self::Err> {
		if self.n_bytes_appended + data.len() > self.mmap_size {
			panic!("Not enough space in mmap for append");
		}

		#[cfg(test)]
		dbg!(&self.fd);

		io::write(&self.fd, data).map_err(|err_no| (Syscall::Write, err_no))?;
		fs::fsync(&self.fd).map_err(|err_no| (Syscall::Fsync, err_no))?;
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
