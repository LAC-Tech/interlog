use crate::unit;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
	pub pos: unit::Byte,
	pub len: unit::Byte,
	pub end: unit::Byte
}

impl Region {
	pub fn new<B: Into<unit::Byte>>(pos: B, len: B) -> Self {
		let pos: unit::Byte = pos.into();
		let len: unit::Byte = len.into();
		Self { pos, len, end: pos + len }
	}

	pub fn from_zero(len: unit::Byte) -> Self {
		Self::new(0.into(), len)
	}

	pub fn zero() -> Self {
		Self::from_zero(0.into())
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

	pub fn write(&self, dest: &mut [u8], src: &[u8]) {
		dest[self.pos.into()..self.end.into()].copy_from_slice(src)
	}

	pub fn empty(&self) -> bool {
		self.len == 0.into()
	}
}
