use core::fmt;
use core::ops;
use core::slice::SliceIndex;

use crate::mem;

/**
 * Fixed Capacity Data Structures.
 *
 * Heavily inspired by Tigerbeetle:
 * https://tigerbeetle.com/blog/a-database-without-dynamic-memory
 */
pub type Res = Result<(), mem::Overrun>;

/**
 * I wrote a 'fresh' implementation, instead of wrapping the std vector.
 * This is so it could be used in a #[no_std] context
 */
#[derive(Clone)]
pub struct Vec<T> {
	items: alloc::boxed::Box<[T]>,
	len: usize,
}

impl<T: core::cmp::PartialEq> PartialEq for Vec<T> {
	fn eq(&self, other: &Self) -> bool {
		self.items[..self.len] == other.items[..other.len]
	}
}

impl<T: fmt::Debug> fmt::Debug for Vec<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entries(self.items.iter().take(self.len)).finish()
	}
}

impl<T: core::default::Default + Clone> Vec<T> {
	pub fn new(capacity: usize) -> Vec<T> {
		let elems = vec![T::default(); capacity].into_boxed_slice();
		Self { items: elems, len: 0 }
	}

	pub fn resize(&mut self, new_len: usize) -> Res {
		self.check_capacity(new_len)?;

		if new_len > self.len {
			self.items[self.len..new_len].fill(T::default());
		}

		self.len = new_len;

		Ok(())
	}

	pub fn fill(&mut self, len: usize, f: impl Fn(&mut [T])) -> Res {
		self.resize(len)?;
		f(&mut self.items[..len]);
		Ok(())
	}

	pub fn pop(&mut self) -> Option<T> {
		(self.len == 0).then(|| {
			self.len -= 1;
			self.items[self.len].clone()
		})
	}
}

impl<T> Vec<T> {
	#[inline]
	pub fn capacity(&self) -> usize {
		self.items.len()
	}

	pub fn from_fn<F>(capacity: usize, cb: F) -> Self
	where
		F: FnMut(usize) -> T,
	{
		let elems = (0..capacity)
			.map(cb)
			.collect::<alloc::vec::Vec<_>>()
			.into_boxed_slice();
		Self { items: elems, len: 0 }
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

	fn check_capacity(&self, requested: usize) -> Res {
		let capacity = self.capacity();
		(capacity >= requested)
			.then_some(())
			.ok_or(mem::Overrun { capacity, requested })
	}

	pub fn push(&mut self, value: T) -> Res {
		self.check_capacity(self.len + 1)?;
		self.items[self.len] = value;
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
		self.items[index] = element;
		Ok(())
	}

	fn get<I>(&self, index: I) -> Option<&<I as SliceIndex<[T]>>::Output>
	where
		I: SliceIndex<[T]>,
	{
		self.items[..self.len].get(index)
	}

	fn last(&self) -> Option<&T> {
		self.len.checked_sub(1).and_then(|i| self.items.get(i))
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
		self.items[self.len..new_len].copy_from_slice(other);
		self.len = new_len;
		Ok(())
	}
}

impl<T> ops::Deref for Vec<T> {
	type Target = [T];

	fn deref(&self) -> &Self::Target {
		&self.items[..self.len]
	}
}

impl<T> ops::DerefMut for Vec<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.items[..self.len]
	}
}

impl AsRef<[u8]> for Vec<u8> {
	fn as_ref(&self) -> &[u8] {
		&self.items[..self.len]
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

impl<T, R> core::ops::Index<R> for Vec<T>
where
	R: core::ops::RangeBounds<usize>,
{
	type Output = [T];
	fn index(&self, index: R) -> &Self::Output {
		let slice: &[T] = &self.items[..self.len];
		&slice[(index.start_bound().cloned(), index.end_bound().cloned())]
	}
}

impl<T, R> core::ops::IndexMut<R> for Vec<T>
where
	R: core::ops::RangeBounds<usize>,
{
	fn index_mut(&mut self, index: R) -> &mut Self::Output {
		let slice: &mut [T] = &mut self.items[..self.len];
		&mut slice[(index.start_bound().cloned(), index.end_bound().cloned())]
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
