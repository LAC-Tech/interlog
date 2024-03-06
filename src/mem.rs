use crate::{unit, util::FixVec};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
	pub pos: unit::Byte,
	pub len: unit::Byte,
	pub end: unit::Byte
}

impl Region {
	pub const fn new<B: Into<unit::Byte>>(pos: B, len: B) -> Self {
		let pos = pos.into();
		let len = len.into();
		Self { pos, len, end: pos + len }
	}

	pub fn from_zero(len: unit::Byte) -> Self {
		Self::new(0.into(), len)
	}

	pub const ZERO: Self = Self::new(0, 0);

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

	pub fn range(&self) -> core::ops::Range<usize> {
		self.pos.into()..self.end.into()
	}
}

pub trait Readable {
	fn as_bytes(&self) -> &[u8];
}

impl Readable for &[u8] {
	fn as_bytes(&self) -> &[u8] {
		self
	}
}

impl Readable for Box<[u8]> {
	fn as_bytes(&self) -> &[u8] {
		self
	}
}

impl Readable for FixVec<u8> {
	fn as_bytes(&self) -> &[u8] {
		&self
	}
}

pub trait Writeable {
	fn as_mut_bytes(&mut self) -> &mut [u8];
}

impl Writeable for FixVec<u8> {
	fn as_mut_bytes(&mut self) -> &mut [u8] {
		self
	}
}

impl Writeable for Box<[u8]> {
	fn as_mut_bytes(&mut self) -> &mut [u8] {
		self
	}
}

pub fn size<R: Readable>(mem: R) -> unit::Byte {
	mem.as_bytes().len().into()
}

pub fn read<R: Readable>(mem: R, r: &Region) -> Option<&[u8]> {
	mem.as_bytes().get(r.range())
}

pub fn write<W: Writeable>(mem: &mut W, r: &Region, data: &[u8]) {
	mem.as_mut_bytes()[r.range()].copy_from_slice(data)
}
