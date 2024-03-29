use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ops;
use core::slice::SliceIndex;

fn uninit_boxed_slice<T>(size: usize) -> Box<[T]> {
	let mut result = Vec::with_capacity(size);
	#[allow(clippy::uninit_vec)]
	unsafe {
		result.set_len(size)
	};
	result.into_boxed_slice()
}

#[derive(Debug)]
pub struct Overflow;
pub type Res = Result<(), Overflow>;

/// Fixed Capacity Vector
/// Tigerstyle: There IS a limit
pub struct FixVec<T> {
	elems: alloc::boxed::Box<[T]>,
	len: usize
}

impl<T> FixVec<T> {
	#[allow(clippy::uninit_vec)]
	pub fn new(capacity: usize) -> FixVec<T> {
		let elems = uninit_boxed_slice(capacity);
		assert_eq!(core::mem::size_of_val(&elems), 16);
		Self { elems, len: 0 }
	}

	#[inline]
	pub fn capacity(&self) -> usize {
		self.elems.len()
	}

	#[inline]
	pub fn len(&self) -> usize {
		self.len
	}

	#[inline]
	pub fn clear(&mut self) {
		self.len = 0;
	}

	fn check_capacity(&self, new_len: usize) -> Res {
		(self.capacity() >= new_len).then_some(()).ok_or(Overflow)
	}

	pub fn push(&mut self, value: T) -> Res {
		self.check_capacity(self.len + 1)?;
		self.elems[self.len] = value;
		Ok(self.len += 1)
	}

	pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) -> Res {
		for elem in iter {
			self.push(elem)?;
		}

		Ok(())
	}

	fn insert(&mut self, index: usize, element: T) -> Res {
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
	pub fn resize(&mut self, new_len: usize, value: T) -> Res {
		self.check_capacity(new_len)?;

		if new_len > self.len {
			self.elems[self.len..new_len].fill(value);
		}

		self.len = new_len;

		Ok(())
	}
}

impl<T: Copy> FixVec<T> {
	pub fn extend_from_slice(&mut self, other: &[T]) -> Res {
		let new_len = self.len + other.len();
		self.check_capacity(new_len)?;
		self.elems[self.len..new_len].copy_from_slice(other);
		self.len = new_len;
		Ok(())
	}
}

impl<T> ops::Deref for FixVec<T> {
	type Target = [T];

	fn deref(&self) -> &Self::Target {
		&self.elems[..self.len]
	}
}

impl<T> ops::DerefMut for FixVec<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.elems[..self.len]
	}
}

impl AsRef<[u8]> for FixVec<u8> {
	fn as_ref(&self) -> &[u8] {
		&self.elems
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use pretty_assertions::assert_eq;

	#[test]
	fn fixvec_stuff() {
		let mut fv = FixVec::<u64>::new(8);
		assert_eq!(fv.capacity(), 8);
		assert_eq!(fv.len(), 0);

		fv.push(42).unwrap();
		assert_eq!(fv.len(), 1);

		assert_eq!(*fv.get(0).unwrap(), 42);

		fv.extend_from_slice(&[6, 1, 9]).unwrap();
		assert_eq!(fv.len, 4);

		assert_eq!(
			fv.into_iter().copied().collect::<Vec<_>>(),
			vec![42, 6, 1, 9]
		);
	}
}
