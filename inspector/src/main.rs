// Compact text representation of bytes to single braille characters
fn u8_to_braille(n: u8) -> char {
	char::from_u32(0x2800 + u32::from(n)).unwrap()
}

/**
 * lh - list as hex
 * lb - list as binary
 * ls - list as string
 *
 * (t)xn write buffer
 * (r)ead cache
 * (e)vents
 */

fn main() {
	let arg = std::env::args().nth(1);
	let log = match arg.as_deref() {
		Some("n") => {
			// TODO: create new actor
		}
		arg => {
			println!("Please provide a valid argument, given {:?}", arg);
			return;
		}
	};
}
