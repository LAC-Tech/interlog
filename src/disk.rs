//! Some error checking around linux sys calls to disk.
use fs::OFlags;
use rustix::fd::AsFd;
use rustix::{fd, fs, io};

type O = OFlags;

/*
pub fn read_from_file(
   bytes: &mut [u8], fd: fd::BorrowedFd, index: &Index
) -> io::Result<()> {
	// need to set len so pread knows how much to fill
	if index.len > self.bytes.capacity() { panic!("OVERFLOW") }
	unsafe {
		self.bytes.set_len(index.len);
	}

	// pread ignores the fd offset, supply your own
	let bytes_read = io::pread(fd, &mut self.bytes, index.pos as u64)?;
	// If this isn't the case, we should figure out why!
	assert_eq!(bytes_read, index.len);

	Ok(())
}
*/

#[derive(Debug, PartialEq)]
pub enum AppendErr {
	OS(rustix::io::Errno),
	NonAtomic { bytes_expected: usize, bytes_written: usize }
}

pub struct Log(fd::OwnedFd);

impl Log {
	pub fn open(path: &str) -> io::Result<Self> {
		let flags = O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC;
		let mode = fs::Mode::RUSR | fs::Mode::WUSR;
		fs::open(path, flags, mode).map(Log)
	}

	pub fn append<B>(&self, bytes: B) -> Result<usize, AppendErr>
	where
		B: AsRef<[u8]>
	{
		let bytes = bytes.as_ref();
		let fd = self.0.as_fd();
		// always sets file offset to EOF.
		let bytes_written = io::write(fd, bytes).map_err(AppendErr::OS)?;
		// Linux 'man open': appending to file opened w/ O_APPEND is atomic
		// TODO: will this happen? if so how to recover?
		let bytes_expected = bytes.len();
		if bytes_written != bytes_expected {
			return Err(AppendErr::NonAtomic { bytes_expected, bytes_written });
		}
		Ok(bytes_written)
	}
}
