use rustsync_core::scanner::scan_dir;

fn main() {
    match scan_dir(".") {
        Ok(entries) => {
            for entry in entries {
                println!("{:?} {:?}", entry.kind, entry.relative_path);
            }
        }
        Err(error) => {
            eprintln!("Failed to scan folder: {error}");
        }
    }
}
