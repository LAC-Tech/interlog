use cursive::theme::{BorderStyle, Palette, Theme};
use cursive::traits::{Nameable, Resizable};
use cursive::views::{Dialog, ListView, SelectView, TextArea};

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

	// q(uit)
	siv.add_global_callback('q', |s| s.quit());
	// a(ppend)
	siv.add_global_callback('a', |s| {
		let dialog = Dialog::new()
			.title("Add an event")
			.content(TextArea::new().fixed_width(50))
			.button("Enqueue & Commit", |_| {});
		s.add_layer(dialog);
	});

	let event_view = SelectView::new().item("0", "item").item("1", "item");
	siv.add_layer(event_view);

	// Starts the event loop.
	siv.run();
}
