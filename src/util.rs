//! Fixed capacity data structures, that do not allocate when modified.
use std::slice::SliceIndex;

//use crate::unit;

fn uninit_boxed_slice<T>(size: usize) -> Box<[T]> {
	let mut result = Vec::with_capacity(size);
	#[allow(clippy::uninit_vec)]
	unsafe {
		result.set_len(size)
	};
	result.into_boxed_slice()
}

/// Fixed Capacity Vector
/// Tigerstyle: There IS a limit
pub struct FixVec<T> {
	elems: alloc::boxed::Box<[T]>,
	len: usize
}

#[derive(Debug)]
pub struct FixVecOverflow;
pub type FixVecRes = Result<(), FixVecOverflow>;

impl<T> FixVec<T> {
	#[allow(clippy::uninit_vec)]
	pub fn new(capacity: usize) -> FixVec<T> {
		let elems = uninit_boxed_slice(capacity);
		assert_eq!(std::mem::size_of_val(&elems), 16);
		Self { elems, len: 0 }
	}

	#[inline]
	pub fn capacity(&self) -> usize {
		self.elems.len()
	}

	#[inline]
	pub fn clear(&mut self) {
		self.len = 0;
	}

	#[inline]
	fn len(&self) -> usize {
		self.len
	}

	fn check_capacity(&self, new_len: usize) -> FixVecRes {
		(self.capacity() >= new_len).then_some(()).ok_or(FixVecOverflow)
	}

	pub fn push(&mut self, value: T) -> FixVecRes {
		self.check_capacity(self.len + 1)?;
		self.elems[self.len + 1] = value;
		Ok(self.len += 1)
	}

	pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) -> FixVecRes {
		for elem in iter {
			self.push(elem)?;
		}

		Ok(())
	}

	fn insert(&mut self, index: usize, element: T) -> FixVecRes {
		self.check_capacity(index + 1)?;
		self.elems[index] = element;
		Ok(())
	}

	fn get<I>(&self, index: I) -> Option<&<I as SliceIndex<[T]>>::Output>
	where
		I: SliceIndex<[T]>
	{
		self.elems[..self.len].get(index)
	}
}

impl<T: Clone + core::fmt::Debug> FixVec<T> {
	pub fn resize(&mut self, new_len: usize, value: T) -> FixVecRes {
		self.check_capacity(new_len)?;

		if new_len > self.len {
			self.elems[self.len..new_len].fill(value);
		}

		self.len = new_len;

		Ok(())
	}
}

impl<T: Copy> FixVec<T> {
	pub fn extend_from_slice(&mut self, other: &[T]) -> FixVecRes {
		let new_len = self.len + other.len();
		self.check_capacity(new_len)?;
		self.elems[self.len..new_len].copy_from_slice(other);
		self.len = new_len;
		Ok(())
	}
}

impl<T> std::ops::Deref for FixVec<T> {
	type Target = [T];

	fn deref(&self) -> &Self::Target {
		&self.elems[..self.len]
	}
}

impl<T> std::ops::DerefMut for FixVec<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.elems[..self.len]
	}
}

impl AsRef<[u8]> for FixVec<u8> {
	fn as_ref(&self) -> &[u8] {
		&self.elems
	}
}
