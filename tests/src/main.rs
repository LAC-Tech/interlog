struct FaultlessStorage(Vec<u8>);

impl FaultlessStorage {
	fn new() -> Self {
		Self(vec![])
	}
}

impl interlog_lib::core::Storage for FaultlessStorage {
	fn append(&mut self, data: &[u8]) {
		self.0.extend(data)
	}

	fn read(&self, buf: &mut [u8], offset: usize) {
		buf.copy_from_slice(&self.0[offset..offset + buf.len()])
	}

	fn size(&self) -> usize {
		self.0.len()
	}
}

mod jagged_vec {
	#[derive(Clone)]
	pub struct JaggedVec<T> {
		elems: Vec<T>,
		offsets: Vec<usize>,
	}

	impl<T> JaggedVec<T> {
		pub fn new() -> Self {
			Self { elems: vec![], offsets: vec![] }
		}

		pub fn iter(&self) -> Iter<T> {
			Iter { elems: &self.elems, offsets: &self.offsets, index: 0 }
		}

		pub fn len(&self) -> usize {
			self.offsets.len()
		}

		pub fn push(&mut self, values: impl IntoIterator<Item = T>) {
			let offset = self.elems.len();
			self.offsets.push(offset);
			self.elems.extend(values);
		}

		pub fn last_mut(&mut self) -> Option<&mut [T]> {
			self.offsets.last().map(|offset| &mut self.elems[*offset..])
		}
	}

	impl<T: Clone> JaggedVec<T> {
		pub fn push_slice(&mut self, values: &[T]) {
			let offset = self.elems.len();
			self.offsets.push(offset);
			self.elems.extend_from_slice(values);
		}
	}

	pub struct Iter<'a, T> {
		elems: &'a [T],
		offsets: &'a [usize],
		index: usize,
	}

	impl<'a, T> Iterator for Iter<'a, T> {
		type Item = &'a [T];

		fn next(&mut self) -> Option<Self::Item> {
			if self.index >= self.offsets.len() {
				return None;
			}

			let start = self.offsets[self.index];
			let end = if self.index + 1 < self.offsets.len() {
				self.offsets[self.index + 1]
			} else {
				self.elems.len()
			};

			self.index += 1;
			Some(&self.elems[start..end])
		}
	}

	impl<'a, T> IntoIterator for &'a JaggedVec<T> {
		type Item = &'a [T];
		type IntoIter = Iter<'a, T>;

		fn into_iter(self) -> Self::IntoIter {
			self.iter()
		}
	}

	#[cfg(test)]
	mod tests {
		use super::*;
		use pretty_assertions::assert_eq;

		#[test]
		fn push_nouns_get_back_compound_noun() {
			let mut nouns = JaggedVec::new();

			nouns.push_slice(b"daten");
			nouns.push_slice(b"traeger");
			nouns.push_slice(b"verwaltung");
			nouns.push_slice(b"system");
			assert_eq!(nouns.elems.len(), 28);
			assert_eq!(nouns.offsets.len(), 4);

			let mut compound_noun = Vec::new();

			for n in &nouns {
				compound_noun.extend(n);
			}

			let actual = b"datentraegerverwaltungsystem";
			let expected = compound_noun.as_slice();
			assert_eq!(actual, expected);
		}

		#[test]
		fn push_with_iterator_check() {
			let mut jv: JaggedVec<u8> = JaggedVec::new();
			assert_eq!(jv.len(), 0);
			jv.push(0..5);
			assert_eq!(jv.len(), 1);
			let expected: &[u8] = &[0, 1, 2, 3, 4];
			assert_eq!(jv.into_iter().next(), Some(expected));
		}
	}
}

#[cfg(test)]
mod tests {
	use std::{fmt, ops};

	use crate::{jagged_vec::JaggedVec, FaultlessStorage};
	use interlog_lib::core::*;
	use pretty_assertions::assert_eq;

	use arbitrary::{Arbitrary, Result, Unstructured};
	use arbtest::arbtest;

	impl<'a, T> arbitrary::Arbitrary<'a> for JaggedVec<T>
	where
		T: arbitrary::Arbitrary<'a> + Default + Clone + 'a,
		&'a [T]: arbitrary::Arbitrary<'a>,
	{
		fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
			let mut jv = JaggedVec::new();
			let outer_len: usize = u.arbitrary_len::<&[T]>()?;

			for _ in 0..outer_len {
				let inner_len = u.arbitrary_len::<T>()?;
				let iter = u.arbitrary_iter::<T>()?;

				jv.push(std::iter::repeat_n(T::default(), inner_len));
				let buf = jv.last_mut().unwrap();

				for (src, dest) in buf.iter_mut().zip(iter) {
					*src = dest?;
				}
			}

			Ok(jv)
		}
	}

	#[test]
	fn empty_commit() {
		let storage = FaultlessStorage::new();
		let mut log = Log::new(Address(0, 0), storage);
		assert_eq!(log.commit(), 0);
	}

	#[test]
	fn empty_read() {
		arbtest(|u| {
			let storage = FaultlessStorage::new();
			let mut log = Log::new(Address(0, 0), storage);
			let bss: JaggedVec<u8> = u.arbitrary()?;
			bss.iter().for_each(|bs| {
				log.enqueue(bs);
			});
			log.commit();
			let mut buf = event::Buf::new();
			log.read_from_end(0, &mut buf);
			assert!(buf.iter().next().is_none());
			Ok(())
		});
	}

	#[test]
	fn rollbacks_are_atomic() {
		arbtest(|u| {
			let storage = FaultlessStorage::new();
			let mut log = Log::new(Address(0, 0), storage);

			let pre_stats = log.stats();
			assert_eq!(pre_stats, Stats { n_events: 0, n_bytes: 0 });
			let bss: JaggedVec<u8> = u.arbitrary()?;

			bss.iter().for_each(|bs| {
				log.enqueue(bs);
			});
			log.clear_enqd();

			let post_stats = log.stats();

			assert_eq!(pre_stats, post_stats);
			Ok(())
		});
	}

	#[test]
	fn enqueue_commit_and_read_data() {
		let storage = FaultlessStorage::new();
		let mut log = Log::new(Address(0, 0), storage);
		let mut read_buf = event::Buf::new();

		let lyrics: [&[u8]; 4] = [
			b"I have known the arcane law",
			b"On strange roads, such visions met",
			b"That I have no fear, nor concern",
			b"For dangers and obstacles of this world",
		];

		{
			assert_eq!(log.enqueue(lyrics[0]), 64);
			assert_eq!(log.commit(), 1);
			log.read_from_end(1, &mut read_buf);
			let actual = read_buf.iter().next().unwrap().payload;
			assert_eq!(actual, lyrics[0]);
		}

		{
			assert_eq!(log.enqueue(lyrics[1]), 72);
			assert_eq!(log.commit(), 1);
			log.read_from_end(1, &mut read_buf);
			let actual = read_buf.iter().next().unwrap().payload;
			assert_eq!(actual, lyrics[1]);
		}

		// Read multiple things from the buffer
		{
			log.read_from_end(2, &mut read_buf);
			let mut it = read_buf.iter();
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[0]);
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[1]);
		}

		// Bulk commit two things
		{
			assert_eq!(log.enqueue(lyrics[2]), 64);
			assert_eq!(log.enqueue(lyrics[3]), 136);
			assert_eq!(log.commit(), 2);

			log.read_from_end(2, &mut read_buf);
			let mut it = read_buf.iter();
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[2]);
			let actual = it.next().unwrap().payload;
			assert_eq!(actual, lyrics[3]);
		}
	}
}

fn main() {
	println!("Hello, world!");
}
