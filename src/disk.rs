use crate::unit;
use rustix::{fd, io};
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
pub enum Err {
	OS(rustix::io::Errno),
	NonAtomic { bytes_expected: unit::Byte, bytes_written: unit::Byte },
}

pub fn write(fd: fd::BorrowedFd, bytes: &[u8]) -> Result<unit::Byte, Err> {
	// always sets file offset to EOF.
	let bytes_written =
		io::write(fd, bytes).map(unit::Byte).map_err(Err::OS)?;
	// Linux 'man open': appending to file opened w/ O_APPEND is atomic
	// TODO: will this happen? if so how to recover?
	let bytes_expected: unit::Byte = bytes.len().into();
	if bytes_written != bytes_expected {
		return Err(Err::NonAtomic { bytes_expected, bytes_written });
	}
	Ok(bytes_written)
}
