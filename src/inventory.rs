//! Observes directory contents without modifying sources or following symlinks.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A regular file observed during traversal; its size may subsequently change.
#[derive(Debug)]
pub struct FileObservation {
    pub path: PathBuf,
    pub size: u64,
}

/// A recoverable failure to observe a path or one of its directory entries.
#[derive(Debug)]
pub struct ReadFailure {
    pub path: PathBuf,
    pub error: io::Error,
}

/// Events emitted by a synchronous scan, including failures that make it incomplete.
#[derive(Debug)]
pub enum Observation {
    File(FileObservation),
    Failure(ReadFailure),
}

/// Whether all directory entries could be observed by a finished traversal.
#[derive(Debug, PartialEq, Eq)]
pub enum ScanOutcome {
    Complete,
    Incomplete,
}

/// Traverses a directory synchronously; callback errors stop traversal immediately.
pub fn scan(
    root: &Path,
    mut observe: impl FnMut(Observation) -> io::Result<()>,
) -> io::Result<ScanOutcome> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "scan root must be a directory, not a file or symlink",
        ));
    }
    let mut pending = vec![root.to_path_buf()];
    let mut outcome = ScanOutcome::Complete;
    while let Some(directory) = pending.pop() {
        // Recheck queued paths: a directory may have disappeared or become a symlink.
        let entries = match fs::symlink_metadata(&directory).and_then(|metadata| {
            if metadata.is_dir() {
                fs::read_dir(&directory)
            } else {
                Err(io::Error::other("directory changed during scan"))
            }
        }) {
            Ok(entries) => entries,
            Err(error) => {
                outcome = ScanOutcome::Incomplete;
                observe(Observation::Failure(ReadFailure {
                    path: directory,
                    error,
                }))?;
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    outcome = ScanOutcome::Incomplete;
                    observe(Observation::Failure(ReadFailure {
                        path: directory.clone(),
                        error,
                    }))?;
                    continue;
                }
            };
            let path = entry.path();
            let result = fs::symlink_metadata(&path);
            match result {
                Ok(metadata) if metadata.is_dir() => pending.push(path),
                Ok(metadata) if metadata.is_file() => {
                    observe(Observation::File(FileObservation {
                        path,
                        size: metadata.len(),
                    }))?;
                }
                Ok(_) => {} // Symlinks and special files are outside this inventory.
                Err(error) => {
                    outcome = ScanOutcome::Incomplete;
                    observe(Observation::Failure(ReadFailure { path, error }))?;
                }
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Each fixture owns its temporary source tree and removes only that tree.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "kartotek-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            // Clock resolution varies; atomically claim a directory and retry collisions.
            for suffix in 0_u64.. {
                let path = base.with_extension(suffix.to_string());
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create test fixture: {error}"),
                }
            }
            unreachable!("test fixture suffixes exhausted")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn empty_directory_is_complete() {
        let fixture = Fixture::new();
        assert_eq!(
            scan(&fixture.0, |_| panic!("unexpected observation")).unwrap(),
            ScanOutcome::Complete
        );
    }

    #[test]
    fn observes_nested_files_and_sizes_without_changing_contents() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("nested")).unwrap();
        fs::write(fixture.0.join("empty"), []).unwrap();
        let nested = fixture.0.join("nested/data");
        fs::write(&nested, b"hello").unwrap();
        let mut files = Vec::new();
        let outcome = scan(&fixture.0, |event| {
            match event {
                Observation::File(file) => files.push((file.path, file.size)),
                Observation::Failure(failure) => panic!("{failure:?}"),
            }
            Ok(())
        })
        .unwrap();
        files.sort();
        assert_eq!(outcome, ScanOutcome::Complete);
        assert_eq!(
            files,
            vec![(fixture.0.join("empty"), 0), (nested.clone(), 5)]
        );
        assert_eq!(fs::read(nested).unwrap(), b"hello");
    }

    #[test]
    fn rejects_missing_roots_and_regular_files() {
        let fixture = Fixture::new();
        assert_eq!(
            scan(&fixture.0.join("missing"), |_| Ok(()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        let file = fixture.0.join("file");
        fs::write(&file, []).unwrap();
        assert_eq!(
            scan(&file, |_| Ok(())).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn callback_failure_stops_scan() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("file"), b"data").unwrap();
        let error = scan(&fixture.0, |_| {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "output closed"))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn disappearing_queued_directory_is_reported_as_incomplete() {
        let fixture = Fixture::new();
        for name in ["a", "b"] {
            fs::create_dir(fixture.0.join(name)).unwrap();
            fs::write(fixture.0.join(name).join("file"), b"data").unwrap();
        }
        let mut removed = None;
        let mut failures = Vec::new();
        let outcome = scan(&fixture.0, |event| {
            match event {
                Observation::File(file) if removed.is_none() => {
                    let sibling = if file.path.parent().unwrap() == fixture.0.join("a") {
                        "b"
                    } else {
                        "a"
                    };
                    let path = fixture.0.join(sibling);
                    fs::remove_dir_all(&path).unwrap();
                    removed = Some(path);
                }
                Observation::File(_) => {}
                Observation::Failure(failure) => failures.push(failure),
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(outcome, ScanOutcome::Incomplete);
        assert_eq!(failures.len(), 1);
        assert_eq!(Some(&failures[0].path), removed.as_ref());
        assert_eq!(failures[0].error.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks_including_cycles_and_broken_targets() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        symlink(&fixture.0, fixture.0.join("cycle")).unwrap();
        symlink(fixture.0.join("missing"), fixture.0.join("broken")).unwrap();
        assert_eq!(
            scan(&fixture.0, |_| panic!("unexpected observation")).unwrap(),
            ScanOutcome::Complete
        );
        assert_eq!(
            scan(&fixture.0.join("cycle"), |_| Ok(()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
