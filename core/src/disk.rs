//! Some error checking around linux sys calls to disk.
use fs::OFlags;
use rustix::fd::AsFd;
use rustix::{fd, fs, io};

use crate::fixed_capacity::Vec;

type O = OFlags;

#[derive(Debug, PartialEq)]
pub enum AppendErr {
	OS(rustix::io::Errno),
	NonAtomic { bytes_expected: usize, bytes_written: usize },
}

#[derive(Debug)]
pub struct Log(fd::OwnedFd);

impl Log {
	pub fn open(path: &str) -> io::Result<Self> {
		let flags = O::DIRECT | O::CREATE | O::APPEND | O::RDWR | O::DSYNC;
		let mode = fs::Mode::RUSR | fs::Mode::WUSR;
		fs::open(path, flags, mode).map(Log)
	}

	pub fn append<B>(&self, bytes: B) -> Result<usize, AppendErr>
	where
		B: AsRef<[u8]>,
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

	/// Returns number of bytes read
	pub fn read(
		&self,
		buf: &mut Vec<u8>,
		byte_offset: usize,
	) -> io::Result<()> {
		let fd = self.0.as_fd();
		let bytes_read = io::pread(fd, buf, byte_offset as u64)?;
		// According to my understanding of the man page this should never
		// happen (famous last words)
		assert_eq!(buf.len(), bytes_read);
		Ok(())
	}
}
