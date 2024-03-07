use crate::{unit, util::FixVec};

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

	pub fn range(&self) -> core::ops::Range<usize> {
		self.pos.into()..self.end.into()
	}

	pub fn empty(&self) -> bool {
		self == &Self::ZERO
	}
}

pub trait Readable<'a> {
	fn as_bytes(self) -> &'a [u8];
}

impl<'a> Readable<'a> for &'a [u8] {
	fn as_bytes(self) -> &'a [u8] {
		self
	}
}

impl<'a> Readable<'a> for &'a Box<[u8]> {
	fn as_bytes(self) -> &'a [u8] {
		self
	}
}

impl<'a> Readable<'a> for &'a FixVec<u8> {
	fn as_bytes(self) -> &'a [u8] {
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

pub fn size<R: AsRef<[u8]>>(mem: R) -> unit::Byte {
	mem.as_ref().len().into()
}

pub fn read<R: AsRef<[u8]>>(mem: R, r: Region) -> Option<&[u8]> {
	mem.as_ref().get(r.range())
}

pub fn write<W: Writeable>(mem: &mut W, r: &Region, data: &[u8]) {
	mem.as_mut_bytes()[r.range()].copy_from_slice(data)
}
