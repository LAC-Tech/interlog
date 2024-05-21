use ncurses::*;

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
	let arg = std::env::args().skip(1).next();
	let log = match arg.as_deref() {
		Some("n") => {
			let config = interlog_core::Config {
				read_cache_capacity: 127,
				key_index_capacity: 0x10000,
				txn_write_buf_capacity: 512,
				disk_read_buf_capacity: 256
			};
			interlog_core::Log::new("/tmp/inspector", config).unwrap()
		}
		arg => {
			println!("Please provide a valid argument, given {:?}", arg);
			return;
		}
	};

	initscr(); /* Start curses mode */
	addstr("Hello World !!!"); /* Print Hello World */
	refresh(); /* Print it on to the real screen */
	getch(); /* Wait for user input */
	endwin(); /* End curses mode */
}
