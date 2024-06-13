use core::fmt;
use core::ops;
use core::slice::SliceIndex;

/**
 * Fixed Capacity Data Structures.
 *
 * Heavily inspired by Tigerbeetle:
 * https://tigerbeetle.com/blog/a-database-without-dynamic-memory
 */
#[derive(Debug)]
pub struct Overflow;
pub type Res = Result<(), Overflow>;

/**
 * I wrote a 'fresh' implementation, instead of wrapping the std vector.
 * This is so it could be used in a #[no_std] context
 */
#[derive(Clone)]
pub struct Vec<T, const CAPACITY: usize> {
	elems: alloc::boxed::Box<[T; CAPACITY]>,
	len: usize,
}

impl<T: fmt::Debug, const CAPACITY: usize> fmt::Debug for Vec<T, CAPACITY> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entries(self.elems.iter().take(self.len)).finish()
	}
}

impl<T: core::default::Default + Clone> Vec<T> {
	pub fn new(capacity: usize) -> Vec<T> {
		let elems = vec![T::default(); capacity].into_boxed_slice();
		Self { elems, len: 0 }
	}

	pub fn from_fn<T, const N: usize, F>(cb: F) -> Self
	where
		F: FnMut(usize) -> T,
	{
	}

	pub fn resize(&mut self, new_len: usize) -> Res {
		self.check_capacity(new_len)?;

		if new_len > self.len {
			self.elems[self.len..new_len].fill(T::default());
		}

		self.len = new_len;

		Ok(())
	}
}

impl<T> Vec<T> {
	#[inline]
	pub fn capacity(&self) -> usize {
		self.elems.len()
	}

	pub fn from_fn<F>(capacity: usize, cb: F) -> Self
	where
		F: FnMut(usize) -> T,
	{
		let elems = (0..capacity)
			.map(cb)
			.collect::<alloc::vec::Vec<_>>()
			.into_boxed_slice();
		Self { elems, len: 0 }
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
		(CAPACITY >= new_len).then_some(()).ok_or(Overflow)
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
		I: SliceIndex<[T]>,
	{
		self.elems[..self.len].get(index)
	}

	fn last1(&self) -> Option<&T> {
		self.len.checked_sub(1).and_then(|i| self.elems.get(i))
	}

	fn last2(&self) -> Option<&T> {
		match self.len {
			0 => None,
			n => self.elems.get(n - 1),
		}
	}

	fn last3(&self) -> Option<&T> {
		if self.len == 0 {
			None
		} else {
			self.elems.get(self.len - 1)
		}
	}
}

impl<T: Copy, const CAPACITY: usize> Vec<T, CAPACITY> {
	pub fn extend_from_slice(&mut self, other: &[T]) -> Res {
		let new_len = self.len + other.len();
		self.check_capacity(new_len)?;
		self.elems[self.len..new_len].copy_from_slice(other);
		self.len = new_len;
		Ok(())
	}
}

impl<T, const CAPACITY: usize> ops::Deref for Vec<T, CAPACITY> {
	type Target = [T];

	fn deref(&self) -> &Self::Target {
		&self.elems[..self.len]
	}
}

impl<T, const CAPACITY: usize> ops::DerefMut for Vec<T, CAPACITY> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.elems[..self.len]
	}
}

impl<const CAPACITY: usize> AsRef<[u8; CAPACITY]> for Vec<u8, CAPACITY> {
	fn as_ref(&self) -> &[u8; CAPACITY] {
		&self.elems
	}
}

/*
macro_rules! vec {
	($($x:expr),*) => {{
		let len = 0 $(+ { let _ = $x; 1 })*;
		let mut temp_vec = Vec::new(len);
		$(temp_vec.push($x);)*
		temp_vec
	}};
}
*/

#[cfg(test)]
mod test {
	use super::*;
	use pretty_assertions::assert_eq;

	#[test]
	fn fixvec_stuff() {
		let mut fv = Vec::<u64>::new(8);
		assert_eq!(fv.len(), 0);

		fv.push(42).unwrap();
		assert_eq!(fv.len(), 1);

		assert_eq!(*fv.get(0).unwrap(), 42);

		fv.extend_from_slice(&[6, 1, 9]).unwrap();
		assert_eq!(fv.len, 4);

		assert_eq!(
			fv.into_iter().copied().collect::<alloc::vec::Vec<_>>(),
			vec![42, 6, 1, 9]
		);
	}
}
