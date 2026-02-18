use std::path::Path;

fn main() {
	let ui_dir = Path::new("../../ui/build");
	if !ui_dir.exists() {
		let _ = std::fs::create_dir_all(ui_dir);
	}
}
