use interlog_core::log::Log;

fn open_file() -> Option<std::path::PathBuf> {
	native_dialog::FileDialog::new()
		.set_location("~/Desktop")
		.add_filter("PNG Image", &["png"])
		.add_filter("JPEG Image", &["jpg", "jpeg"])
		.show_open_single_file()
		.unwrap()
}

fn main() {
	let window = MainWindow::new().unwrap()
    
    window.run().unwrap();
}

slint::slint! {
	import { Button } from "std-widgets.slint";
    export global Logic {
        
    }
	export component MainWindow inherits Window {
		callback open_file;
		preferred-height: 600px;
		preferred-width: 800px;
		default-font-size: 25px;
		Button {
			text: "New Log";
			clicked => {
				self.text = "clicked";
			}
		}
	}
}
