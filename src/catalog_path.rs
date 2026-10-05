//! Resolves catalog destinations without permitting writes into a source tree.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Resolves a directory root while rejecting a symlink at the supplied root.
pub fn source_root(root: &Path) -> io::Result<PathBuf> {
    if !fs::symlink_metadata(root)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "scan root must be a directory, not a file or symlink",
        ));
    }
    fs::canonicalize(root)
}

/// Resolves a catalog and checks existing files that SQLite may write alongside it.
pub fn destination(path: &Path, source: Option<&Path>) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "catalog must name a file"))?;
    let parent = path.parent().filter(|path| !path.as_os_str().is_empty());
    let parent = fs::canonicalize(parent.unwrap_or(Path::new(".")))?;
    if source.is_some_and(|source| parent.starts_with(source)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "catalog must be outside the scanned directory",
        ));
    }
    let catalog = parent.join(name);
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let mut name = catalog.as_os_str().to_os_string();
        name.push(suffix);
        match fs::symlink_metadata(Path::new(&name)) {
            Ok(metadata) if !metadata.is_file() || metadata.nlink() != 1 => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "catalog and SQLite sidecars must be regular files without links",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(catalog)
}
