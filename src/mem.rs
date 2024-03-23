#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
	pub pos: usize,
	pub len: usize
}

#[derive(Debug)]
pub enum WriteErr {
	DestOverflow,
	LenMisMatch
}

#[derive(Debug)]
pub struct ExtendOverflow;

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

		let dest_start: usize = self.pos;
		let dest_end: usize = self.end();

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

	pub fn extend(
		&mut self,
		dest: &mut [u8],
		src: &[u8]
	) -> Result<(), ExtendOverflow> {
		let extension = Region::new(self.len, src.len());

		if let Err(err) = extension.write(dest, src) {
			match err {
				WriteErr::DestOverflow => return Err(ExtendOverflow),
				WriteErr::LenMisMatch => panic!("assumed regions match up")
			}
		}

		self.lengthen(src.len());
		Ok(())
	}

	pub fn empty(&self) -> bool {
		self.len == 0
	}
}
