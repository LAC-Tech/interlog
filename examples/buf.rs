extern crate interlog;

use interlog::{mem, unit};
use pretty_assertions::assert_eq;

struct Buf {
	mem: Box<[u8]>,
	a: mem::Region,
	b: mem::Region
}

impl Buf {
	fn new(capacity: usize) -> Self {
		let mem = vec![0; capacity].into_boxed_slice();
		let a = mem::Region::ZERO;
		let b = mem::Region::ZERO; // by definition B always starts at 0
		Self { mem, a, b }
	}

	fn extend(&mut self, s: &[u8]) -> Result<(), mem::ExtendOverflow> {
		match (self.a.empty(), self.b.empty()) {
			(true, true) => self.a.extend(&mut self.mem, s),
			(false, true) => match self.a.extend(&mut self.mem, s) {
				Ok(()) => Ok(()),
				Err(mem::ExtendOverflow) => self.overlapping_write(s)
			},
			(_, false) => self.overlapping_write(s)
		}
	}

	fn overlapping_write(
		&mut self,
		s: &[u8]
	) -> Result<(), mem::ExtendOverflow> {
		match self.new_a_pos(s) {
			// Truncate A and write to B
			Some(new_a_pos) => {
				self.a.change_pos(new_a_pos);
				self.b.extend(self.mem.as_mut(), s)
			}
			// We've searched past the end of A and found nothing.
			// B is now A
			None => {
				self.a = mem::Region::new(0.into(), self.b.end);
				self.b = mem::Region::ZERO;
				match self.a.extend(self.mem.as_mut(), s) {
					Ok(()) => Ok(()),
					// the new A cannot fit, erase it.
					// would not occur with buf size 2x or more max event size
					Err(mem::ExtendOverflow) => {
						self.a = mem::Region::ZERO;
						self.a.extend(&mut self.mem, s)
					}
				}
			}
		}
	}

	fn new_a_pos(&self, es: &[u8]) -> Option<unit::Byte> {
		let new_b_end = self.b.end + mem::size(es);

		self.read_a().into_iter().skip(new_b_end.into()).enumerate().find_map(
			|(i, &c)| is_upper_ascii(c).then(|| unit::Byte(i) + new_b_end)
		)
	}

	fn read_a(&self) -> &[u8] {
		self.a.read(&self.mem).expect("a range to be correct")
	}

	fn read_b(&self) -> &[u8] {
		self.b.read(&self.mem).expect("b range to be correct")
	}
}

fn is_upper_ascii(ch: u8) -> bool {
	ch >= 65 && ch <= 90
}

fn main() {
	let mut buf = Buf::new(16);

	buf.extend(b"Who").unwrap();
	assert_eq!(buf.a, mem::Region::new(0, 3));
	assert_eq!(buf.b, mem::Region::ZERO);
	assert_eq!(buf.read_a(), b"Who");
	assert_eq!(buf.read_b(), b"");

	buf.extend(b"Is").unwrap();
	assert_eq!(buf.a, mem::Region::new(0, 5));
	assert_eq!(buf.b, mem::Region::ZERO);
	assert_eq!(buf.read_a(), b"WhoIs");
	assert_eq!(buf.read_b(), b"");

	buf.extend(b"This").unwrap();
	assert_eq!(buf.a, mem::Region::new(0, 9));
	assert_eq!(buf.b, mem::Region::ZERO);
	assert_eq!(buf.read_a(), b"WhoIsThis");
	assert_eq!(buf.read_b(), b"");

	buf.extend(b"Doin").unwrap();
	assert_eq!(buf.a, mem::Region::new(0, 13));
	assert_eq!(buf.b, mem::Region::ZERO);
	assert_eq!(buf.read_a(), b"WhoIsThisDoin");
	assert_eq!(buf.read_b(), b"");

	buf.extend(b"This").unwrap();
	assert_eq!(buf.a, mem::Region::new(5, 8));
	assert_eq!(buf.b, mem::Region::new(0, 4));
	assert_eq!(buf.read_a(), b"ThisDoin");
	assert_eq!(buf.read_b(), b"This");

	buf.extend(b"Synthetic").unwrap();
	assert_eq!(buf.a, mem::Region::new(0, 13));
	assert_eq!(buf.b, mem::Region::ZERO);
	assert_eq!(buf.read_a(), b"ThisSynthetic");
	assert_eq!(buf.read_b(), b"");

	buf.extend(b"Type").unwrap();
	assert_eq!(buf.a, mem::Region::new(4, 9));
	assert_eq!(buf.b, mem::Region::new(0, 4));
	assert_eq!(buf.read_a(), b"Synthetic");
	assert_eq!(buf.read_b(), b"Type");

	buf.extend(b"Of").unwrap();
	assert_eq!(buf.a, mem::Region::new(0, 6));
	assert_eq!(buf.b, mem::Region::ZERO);
	assert_eq!(buf.read_a(), b"TypeOf");
	assert_eq!(buf.read_b(), b"");

	buf.extend(b"Alpha").unwrap();
	assert_eq!(buf.read_a(), b"TypeOfAlpha");
	assert_eq!(buf.read_b(), b"");
	assert_eq!(buf.a, mem::Region::new(0, 11));
	assert_eq!(buf.b, mem::Region::ZERO);

	buf.extend(b"Beta").unwrap();
	assert_eq!(buf.a, mem::Region::new(0, 15));
	assert_eq!(buf.b, mem::Region::ZERO);
	assert_eq!(buf.read_a(), b"TypeOfAlphaBeta");
	assert_eq!(buf.read_b(), b"");

	buf.extend(b"Psychedelic").unwrap();
	assert_eq!(buf.a, mem::Region::new(11, 4));
	assert_eq!(buf.b, mem::Region::new(0, 11));
	assert_eq!(buf.read_a(), b"Beta");
	assert_eq!(buf.read_b(), b"Psychedelic");

	buf.extend(b"Funkin").unwrap();
	assert_eq!(buf.a, mem::Region::new(0, 6));
	assert_eq!(buf.b, mem::Region::ZERO);
	assert_eq!(buf.read_a(), b"Funkin");
	assert_eq!(buf.read_b(), b"");
}
