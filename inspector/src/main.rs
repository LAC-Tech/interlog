use cursive::theme::{BorderStyle, Palette, Theme};
use cursive::views::{Dialog, ListView, SelectView, TextView};

// Compact text representation of bytes to single braille characters
fn u8_to_braille(n: u8) -> char {
	char::from_u32(0x2800 + u32::from(n)).unwrap()
}

fn main() {
	let arg = std::env::args().skip(1).next();
	let log = match arg.as_deref() {
		Some("n") => {
			let config = interlog_core::Config {
				read_cache_capacity: 127,
				key_index_capacity: 0x10000,
				txn_write_buf_capacity: 512,
				disk_read_buf_capacity: 256,
			};
			interlog_core::Log::new("/tmp/inspector", config).unwrap();
		}
		arg => {
			println!("Please provide a valid argument, given {:?}", arg);
			return;
		}
	};
	// Creates the cursive root - required for every application.
	let mut siv = cursive::default();

	siv.set_theme(Theme {
		shadow: false,
		borders: BorderStyle::Simple,
		palette: Palette::terminal_default(),
	});

	siv.add_global_callback('q', |s| s.quit());

	siv.add_layer(SelectView::new().item("0", "item").item("1", "item"));

	// Starts the event loop.
	siv.run();
}
