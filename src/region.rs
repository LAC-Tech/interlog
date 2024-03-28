#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
	pub pos: usize,
	pub len: usize
}

#[derive(Debug)]
pub struct WriteErr;

impl Region {
	pub fn new(pos: usize, len: usize) -> Self {
		Self { pos, len }
	}

	pub const ZERO: Self = Self { pos: 0, len: 0 };

	pub fn lengthen(&mut self, n: usize) {
		self.len += n;
	}

	pub fn end(&self) -> usize {
		self.pos + self.len
	}

	/// Set pos to new_pos, while leaving the end the same
	pub fn change_pos(&mut self, new_pos: usize) {
		self.pos = new_pos;
		self.len = self.end() - new_pos;
	}

	pub fn next(&self, len: usize) -> Self {
		Self::new(self.end(), len)
	}

	pub fn read<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
		bytes.get(self.pos..self.end())
	}

	pub fn write(&self, dest: &mut [u8], src: &[u8]) -> Result<(), WriteErr> {
		// I tried using_copy_from slice, but it panics on overflow
		// I need an error, so I basically reimplemented it
		if self.end() > dest.len() {
			return Err(WriteErr);
		}

		let dest = &mut dest[self.pos..self.end()];

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
		self.len == 0
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use pretty_assertions::assert_eq;

	#[test]
	fn empty_region_empty_slice() {
		let r = Region::ZERO;
		assert_eq!(r.read(&[]), Some([].as_slice()));
	}

	#[test]
	fn empty_region_non_empty_slice() {
		let r = Region::ZERO;
		assert_eq!(r.read(&[1, 3, 3, 7]), Some([].as_slice()));
	}

	#[test]
	fn non_empty_region_non_empty_slice() {
		let r = Region::new(1, 2);
		assert_eq!(r.read(&[1, 3, 3, 7]), Some([3, 3].as_slice()));
	}
}
