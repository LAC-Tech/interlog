use core::cmp::Ordering;

use cursive::theme::{BorderStyle, Palette, Theme};
use cursive::views::{Dialog, TextView};
use cursive_table_view::{TableView, TableViewItem};

// Provide a type for the table's columns
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
enum BasicColumn {
	Origin,
	LogicalPos,
	ByteLen,
	Payload
}

impl TableViewItem<BasicColumn> for interlog_core::event::Event<'_> {
	fn to_column(&self, column: BasicColumn) -> String {
		match column {
			BasicColumn::Origin => self.id.origin.to_string(),
			BasicColumn::LogicalPos => self.id.logical_pos.to_string(),
			BasicColumn::ByteLen => self.payload.len().to_string(),
			BasicColumn::Payload => {
				self.payload.iter().map(|byte| format!("{:x}", byte)).collect()
			}
		}
	}

	fn cmp(&self, other: &Self, column: BasicColumn) -> Ordering
	where
		Self: Sized
	{
		match column {
			BasicColumn::Origin => self.id.origin.cmp(&other.id.origin),
			BasicColumn::LogicalPos => {
				self.id.logical_pos.cmp(&other.id.logical_pos)
			}
			BasicColumn::ByteLen => self.payload.len().cmp(&self.payload.len()),
			BasicColumn::Payload => self.payload.cmp(other.payload)
		}
	}
}

fn main() {
	for n in 0..=0xff {
		println!("{}", char::from_u32(0x2800 + n).unwrap());
	}
	/*
	// Creates the cursive root - required for every application.
	let mut siv = cursive::default();

	siv.set_theme(Theme {
		shadow: false,
		borders: BorderStyle::Simple,
		palette: Palette::terminal_default()
	});

	// Creates a dialog with a single "Quit" button
	siv.add_layer(
		Dialog::around(TextView::new("Hello Dialog!"))
			.title("Cursive")
			.button("Quit", |s| s.quit())
	);

	// Starts the event loop.
	siv.run();
	*/
}
