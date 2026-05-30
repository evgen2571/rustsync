use crate::metadata::EntryKind;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ScanEntry {
    pub relative_path: PathBuf,
    pub kind: EntryKind,
}

pub fn scan_dir(root: impl AsRef<Path>) -> io::Result<Vec<ScanEntry>> {
    let root = root.as_ref().canonicalize()?;
    let mut entries = Vec::new();

    for item in WalkDir::new(&root) {
        let item = item?;

        let path = item.path();
        if path == root {
            continue;
        }

        let relative_path = path.strip_prefix(&root).unwrap().to_path_buf();
        let metadata = fs::metadata(path)?;

        if metadata.is_dir() {
            entries.push(ScanEntry {
                relative_path,
                kind: EntryKind::Dir,
            });

            continue;
        }

        if metadata.is_file() {
            entries.push(ScanEntry {
                relative_path,
                kind: EntryKind::File,
            });
        }
    }

    Ok(entries)
}
