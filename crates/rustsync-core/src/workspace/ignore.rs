use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use super::{WORKSPACE_DIR, Workspace};
use crate::manifest::load_manifest;

pub(crate) struct IgnoreRules {
    matcher: Gitignore,
    root: PathBuf,
    tracked: BTreeSet<PathBuf>,
}

impl IgnoreRules {
    pub(crate) fn load(workspace: &Workspace) -> io::Result<Self> {
        let root = workspace.layout.root.canonicalize()?;
        let path = root.join(".rustsyncignore");
        let mut builder = GitignoreBuilder::new(&root);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                if let Some(error) = builder.add(&path) {
                    return Err(io::Error::other(error));
                }
            }
            Ok(_) => return Err(io::Error::other(".rustsyncignore must be a regular file")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut tracked = BTreeSet::new();
        if let Some(manifest) = load_manifest(workspace).map_err(io::Error::other)? {
            for path in manifest.entries.keys() {
                tracked.extend(Path::new(path).ancestors().map(Path::to_path_buf));
            }
        }
        Ok(Self {
            matcher: builder.build().map_err(io::Error::other)?,
            root,
            tracked,
        })
    }

    pub(crate) fn excludes(&self, relative: &Path, is_dir: bool) -> bool {
        is_internal(relative)
            || (!self.tracked.contains(relative)
                && self
                    .matcher
                    .matched_path_or_any_parents(self.root.join(relative), is_dir)
                    .is_ignore())
    }
}

pub(crate) fn is_internal(relative: &Path) -> bool {
    relative.components().any(|component| {
        let name = component.as_os_str();
        name == WORKSPACE_DIR || name.to_string_lossy().starts_with(".rustsync-tmp-")
    })
}
