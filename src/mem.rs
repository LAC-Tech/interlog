use crate::unit;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
	pub pos: unit::Byte,
	pub len: unit::Byte,
	pub end: unit::Byte
}

#[derive(Debug)]
pub enum WriteErr {
	DestOverflow,
	LenMisMatch
}

pub type WriteRes = Result<(), WriteErr>;

impl Region {
	pub fn new<B: Into<unit::Byte>>(pos: B, len: B) -> Self {
		let pos: unit::Byte = pos.into();
		let len: unit::Byte = len.into();
		Self { pos, len, end: pos + len }
	}

	pub const ZERO: Self =
		Self { pos: unit::Byte(0), len: unit::Byte(0), end: unit::Byte(0) };

	pub fn lengthen(&mut self, n: unit::Byte) {
		self.len += n;
		self.end = self.pos + self.len;
	}

	/// Set pos to new_pos, while leaving the end the same
	pub fn change_pos(&mut self, new_pos: unit::Byte) {
		self.pos = new_pos;
		self.len = self.end - new_pos;
	}

	pub fn next(&self, len: unit::Byte) -> Self {
		Self::new(self.end, len)
	}

	pub fn read<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
		bytes.get(self.pos.into()..self.end.into())
	}

	pub fn write(&self, dest: &mut [u8], src: &[u8]) -> WriteRes {
		// I tried using_copy_from slice, but it panics on overflow
		// I need an error, so I basically reimplemented it

		//let dest = &mut dest[self.pos.into()..self.end.into()];

		let dest_start: usize = self.pos.into();
		let dest_end: usize = self.end.into();

		if dest_end > dest.len() {
			return Err(WriteErr::DestOverflow);
		}

		let dest = &mut dest[dest_start..dest_end];

		if dest.len() != src.len() {
			return Err(WriteErr::LenMisMatch);
		}

		// We know that src and dest are the same length
		unsafe {
			core::ptr::copy_nonoverlapping(
				src.as_ptr(),
				dest.as_mut_ptr(),
				src.len()
			);
		}

		Ok(())
	}

	pub fn empty(&self) -> bool {
		self.len == 0.into()
	}
}

// Looks a bit siller but easier than intermediate vars and into everywhere
pub fn size<T: AsRef<[u8]>>(bytes: T) -> unit::Byte {
	bytes.as_ref().len().into()
}
