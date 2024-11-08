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
}

#[cfg(test)]
mod tests {
	use super::*;
	use interlog_lib::core::*;
	use pretty_assertions::assert_eq;
	use proptest::prelude::*;

	#[test]
	fn empty_commit() {
		let storage = FaultlessStorage::new();
		let mut log = Log::new(Address(0, 0), storage);
		assert_eq!(log.commit(), 0);
	}

	proptest! {
		#[test]
		fn empty_read(bytes in proptest::collection::vec(any::<u8>(), 1..100)) {
			let storage = FaultlessStorage::new();
			let mut log = Log::new(Address(0, 0), storage);
			log.enqueue(&bytes);
			log.commit();

			let mut buf = event::Buf::new();
			assert!(buf.iter().next().is_none());
			log.read_from_end(0, &mut buf);
			assert!(buf.iter().next().is_none());
		}
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
