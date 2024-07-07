use core::fmt;
use core::ops;
use core::slice::SliceIndex;

/**
 * Fixed Capacity Data Structures.
 *
 * Heavily inspired by Tigerbeetle:
 * https://tigerbeetle.com/blog/a-database-without-dynamic-memory
 */
#[derive(Debug, PartialEq)]
pub struct Overrun;
pub type Res = Result<(), Overrun>;

/**
 * I wrote a 'fresh' implementation, instead of wrapping the std vector.
 * This is so it could be used in a #[no_std] context
 */
#[derive(Clone)]
pub struct Vec<T> {
	elems: alloc::boxed::Box<[T]>,
	len: usize,
}

impl<T: core::cmp::PartialEq> PartialEq for Vec<T> {
	fn eq(&self, other: &Self) -> bool {
		self.elems[..self.len] == other.elems[..other.len]
	}
}

impl<T: fmt::Debug> fmt::Debug for Vec<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entries(self.elems.iter().take(self.len)).finish()
	}
}

impl<T: core::default::Default + Clone> Vec<T> {
	pub fn new(capacity: usize) -> Vec<T> {
		let elems = vec![T::default(); capacity].into_boxed_slice();
		Self { elems, len: 0 }
	}

	pub fn resize(&mut self, new_len: usize) -> Res {
		self.check_capacity(new_len)?;

		if new_len > self.len {
			self.elems[self.len..new_len].fill(T::default());
		}

		self.len = new_len;

		Ok(())
	}

	pub fn pop(&mut self) -> Option<T> {
		(self.len == 0).then(|| {
			self.len -= 1;
			self.elems[self.len].clone()
		})
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
	pub fn is_empty(&self) -> bool {
		self.len == 0
	}

	#[inline]
	pub fn clear(&mut self) {
		self.len = 0;
	}

	fn check_capacity(&self, new_len: usize) -> Res {
		(self.capacity() >= new_len).then_some(()).ok_or(Overrun)
	}

	pub fn push(&mut self, value: T) -> Res {
		self.check_capacity(self.len + 1)?;
		self.elems[self.len] = value;
		self.len += 1;
		Ok(())
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

	fn last(&self) -> Option<&T> {
		self.len.checked_sub(1).and_then(|i| self.elems.get(i))
	}

	pub fn can_be_extended_by(&self, other: &[T]) -> bool {
		let new_len = self.len + other.len();
		self.check_capacity(new_len).is_ok()
	}
}

impl<T: Copy> Vec<T> {
	pub fn extend_from_slice(&mut self, other: &[T]) -> Res {
		let new_len = self.len + other.len();
		self.check_capacity(new_len)?;
		self.elems[self.len..new_len].copy_from_slice(other);
		self.len = new_len;
		Ok(())
	}
}

impl<T> ops::Deref for Vec<T> {
	type Target = [T];

	fn deref(&self) -> &Self::Target {
		&self.elems[..self.len]
	}
}

impl<T> ops::DerefMut for Vec<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.elems[..self.len]
	}
}

impl AsRef<[u8]> for Vec<u8> {
	fn as_ref(&self) -> &[u8] {
		&self.elems[..self.len]
	}
}

impl<'a, T> IntoIterator for &'a Vec<T> {
	type Item = &'a T;
	type IntoIter = core::slice::Iter<'a, T>;

	fn into_iter(self) -> Self::IntoIter {
		self[..self.len].iter()
	}
}

impl<'a, T> IntoIterator for &'a mut Vec<T> {
	type Item = &'a T;
	type IntoIter = core::slice::Iter<'a, T>;

	fn into_iter(self) -> Self::IntoIter {
		self[..self.len].iter()
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
