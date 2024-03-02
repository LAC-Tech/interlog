extern crate interlog;

use interlog::util::*;
use pretty_assertions::assert_eq;

struct Buf {
	mem: Box<[u8]>,
	a: Segment,
	b_end: usize
}

impl Buf {
	fn new(capacity: usize) -> Self {
		let mem = vec![0; capacity].into_boxed_slice();
		let a = Segment::ZERO;
		let b_end = 0; // by definition B always starts at 0
		Self { mem, a, b_end }
	}

	fn extend(&mut self, s: &[u8]) {
		let a_would_overflow = self.a.end + s.len() > self.mem.len();

		if !a_would_overflow && self.b_end == 0 {
			self.write_a(s);
			return;
		}

		let a_will_be_modified = self.b_end + s.len() >= self.a.pos;

		if !a_will_be_modified {
			self.write_b(s);
			return;
		}

		let b_would_overflow = self.b_end + s.len() > self.mem.len();

		if b_would_overflow {
			self.a = Segment::new(0, self.b_end);
			self.b_end = 0;
		}

		let new_b_end = self.b_end + s.len();

		// EITHER part of A will be overwritten by extending B,
		// OR extending B would overflow, so instead we start a new A
		let new_a_pos: Option<usize> = self
			.read_a()
			.into_iter()
			.skip(new_b_end)
			.enumerate()
			.find_map(|(i, &c)| is_upper_ascii(c).then(|| i + new_b_end));

		match new_a_pos {
			// Truncate A and write to B
			Some(new_a_pos) => {
				self.a.change_pos(new_a_pos);
				self.write_b(s);
			}
			// We've searched past the end of A and found nothing.
			// B is now A
			None => {
				self.a = Segment::new(0, self.b_end);
				self.b_end = 0;
				self.write_a(s);
			}
		}
	}

	fn write_a(&mut self, s: &[u8]) {
		self.mem[self.a.len..self.a.len + s.len()].copy_from_slice(s);
		self.a.lengthen(s.len());
	}

	fn write_b(&mut self, s: &[u8]) {
		self.mem[self.b_end..self.b_end + s.len()].copy_from_slice(s);
		self.b_end += s.len();
	}

	fn read_a(&self) -> &[u8] {
		&self.mem[self.a.range()]
	}

	fn read_b(&self) -> &[u8] {
		&self.mem[0..self.b_end]
	}
}

fn is_upper_ascii(ch: u8) -> bool {
	ch >= 65 && ch <= 90
}

fn main() {
	let mut buf = Buf::new(16);

	buf.extend(b"Who");
	assert_eq!(buf.read_a(), b"Who");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 3));
	assert_eq!(buf.b_end, 0);

	buf.extend(b"Is");
	assert_eq!(buf.read_a(), b"WhoIs");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 5));
	assert_eq!(buf.b_end, 0);

	buf.extend(b"This");
	assert_eq!(buf.read_a(), b"WhoIsThis");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 9));
	assert_eq!(buf.b_end, 0);

	buf.extend(b"Doin");
	assert_eq!(buf.read_a(), b"WhoIsThisDoin");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 13));
	assert_eq!(buf.b_end, 0);

	buf.extend(b"This");
	assert_eq!(buf.read_a(), b"ThisDoin");
	assert_eq!(buf.read_b(), b"This");
	assert_eq!(buf.a, Segment::new(5, 8));
	assert_eq!(buf.b_end, 4);

	buf.extend(b"Synthetic");
	assert_eq!(buf.read_a(), b"ThisSynthetic");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 13));
	assert_eq!(buf.b_end, 0);

	buf.extend(b"Type");
	assert_eq!(buf.read_a(), b"Synthetic");
	assert_eq!(buf.read_b(), b"Type");
	assert_eq!(buf.a, Segment::new(4, 9));
	assert_eq!(buf.b_end, 4);

	buf.extend(b"Of");
	assert_eq!(buf.read_a(), b"TypeOf");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 6));
	assert_eq!(buf.b_end, 0);

	buf.extend(b"Alpha");
	assert_eq!(buf.read_a(), b"TypeOfAlpha");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 11));
	assert_eq!(buf.b_end, 0);

	buf.extend(b"Beta");
	assert_eq!(buf.read_a(), b"TypeOfAlphaBeta");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 15));
	assert_eq!(buf.b_end, 0);

	buf.extend(b"Psychedelic");
	assert_eq!(buf.read_a(), b"Beta");
	assert_eq!(buf.read_b(), b"Psychedelic");
	assert_eq!(buf.a, Segment::new(11, 4));
	assert_eq!(buf.b_end, 11);

	buf.extend(b"Funkin");
	assert_eq!(buf.read_a(), b"Funkin");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, Segment::new(0, 6));
	assert_eq!(buf.b_end, 0);
}
