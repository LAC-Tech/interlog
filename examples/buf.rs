extern crate interlog;

use interlog::util::*;
use interlog::*;
use pretty_assertions::assert_eq;

struct Buf {
	mem: Box<[u8]>,
	a: Segment,
	b: Segment
}

impl Buf {
	fn new(capacity: usize) -> Self {
		let mem = vec![0; capacity].into_boxed_slice();
		let a = Segment::ZERO;
		let b = Segment::ZERO;
		Self { mem, a, b }
	}

	fn extend(&mut self, s: &[u8]) {
		if self.a.end + s.len() > self.mem.len() {
			let new_b_end = self.b.end + s.len();

			// No overwriting of a will occur
			if self.a.pos > new_b_end {
				self.write_b(s);
				return;
			}

			// Part of A will be overwritten. We need to truncate it to find
			// the first point that is the beginning of a new world
			let new_a_pos: Option<usize> = self
				.read_a()
				.into_iter()
				.skip(self.b.end)
				.enumerate()
				.find_map(|(i, &c)| is_upper_ascii(c).then(|| i + self.a.pos));

			match new_a_pos {
				// Truncate A and write to B
				Some(new_a_pos) => {
					self.a.change_pos(new_a_pos);
					self.write_b(s);
				}
				// We've searched past the end of A and found nothing.
				// B is now A
				None => {
					self.a = self.b;
					self.b = Segment::ZERO;
					self.write_a(s);
				}
			}
		} else {
			self.mem[self.a.len..self.a.len + s.len()].copy_from_slice(s);
			self.a.lengthen(s.len());
		}
	}

	fn write_a(&mut self, s: &[u8]) {
		self.mem[self.a.len..self.a.len + s.len()].copy_from_slice(s);
		self.a.lengthen(s.len());
	}

	fn write_b(&mut self, s: &[u8]) {
		self.mem[self.b.len..self.b.len + s.len()].copy_from_slice(s);
		self.b.lengthen(s.len());
	}

	fn read_a(&self) -> &[u8] {
		&self.mem[self.a.range()]
	}
}

fn is_upper_ascii(ch: u8) -> bool {
	ch >= 65 && ch <= 90
}

fn main() {
	let mut buf = Buf::new(16);

	buf.extend(b"Who");
	assert_eq!(buf.a, Segment::new(0, 3));
	assert_eq!(buf.b, Segment::ZERO);
	assert_eq!(buf.read_a(), b"Who");

	buf.extend(b"Is");
	assert_eq!(buf.a, Segment::new(0, 5));
	assert_eq!(buf.b, Segment::ZERO);
	assert_eq!(buf.read_a(), b"WhoIs");

	buf.extend(b"This");
	assert_eq!(buf.a, Segment::new(0, 9));
	assert_eq!(buf.b, Segment::ZERO);
	assert_eq!(buf.read_a(), b"WhoIsThis");

	buf.extend(b"Doin");
	assert_eq!(buf.a, Segment::new(0, 13));
	assert_eq!(buf.b, Segment::ZERO);
	assert_eq!(buf.read_a(), b"WhoIsThisDoin");

	//buf.extend(b"This");
	//assert_eq!(buf.a, Segment::new(5, 8));
	//assert_eq!(buf.b, Some(Segment::new(0, 4)));
}
