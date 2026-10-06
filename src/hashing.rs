//! Hashes bounded file observations while rejecting changed or replaced sources.

use crate::inventory::{FileMetadata, FileObservation};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;

pub const PREFIX_BYTES: u64 = 64 * 1024;

/// A SHA-256 digest of an observed byte sequence.
pub type ContentHash = [u8; 32];

fn changed() -> io::Error {
    io::Error::other("file changed or was replaced since its metadata observation")
}

fn validate(observation: &FileObservation, opened: Option<&File>) -> io::Result<()> {
    let current = fs::symlink_metadata(&observation.path)?;
    if !current.is_file() || FileMetadata::observe(&current) != observation.metadata {
        return Err(changed());
    }
    if let Some(file) = opened {
        let current = file.metadata()?;
        if !current.is_file() || FileMetadata::observe(&current) != observation.metadata {
            return Err(changed());
        }
    }
    Ok(())
}

fn open(observation: &FileObservation) -> io::Result<File> {
    validate(observation, None)?;
    // A replaced symlink must not be followed; a replaced FIFO must not block open.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&observation.path)?;
    validate(observation, Some(&file))?;
    Ok(file)
}

fn eof(file: &mut File) -> io::Result<()> {
    let mut extra = [0];
    if file.read(&mut extra)? != 0 {
        return Err(changed());
    }
    Ok(())
}

/// Reads at most 64 KiB; for smaller files the prefix is also the full-file digest.
pub fn prefix(observation: &FileObservation) -> io::Result<ContentHash> {
    let mut file = open(observation)?;
    let length = observation.metadata.size.min(PREFIX_BYTES) as usize;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    if observation.metadata.size <= PREFIX_BYTES {
        eof(&mut file)?;
    }
    validate(observation, Some(&file))?;
    Ok(Sha256::digest(&bytes).into())
}

/// Streams exactly the observed size and checks that its prefix still matches the first pass.
pub fn full(
    observation: &FileObservation,
    expected_prefix: ContentHash,
) -> io::Result<ContentHash> {
    let mut file = open(observation)?;
    let mut remaining = observation.metadata.size;
    let mut prefix_remaining = remaining.min(PREFIX_BYTES);
    let mut buffer = [0; 64 * 1024];
    let mut full = Sha256::new();
    let mut prefix = Sha256::new();
    while remaining > 0 {
        let length = remaining.min(buffer.len() as u64) as usize;
        let count = file.read(&mut buffer[..length])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "file shortened during hashing",
            ));
        }
        full.update(&buffer[..count]);
        let prefix_count = prefix_remaining.min(count as u64) as usize;
        prefix.update(&buffer[..prefix_count]);
        prefix_remaining -= prefix_count as u64;
        remaining -= count as u64;
    }
    eof(&mut file)?;
    validate(observation, Some(&file))?;
    let observed_prefix: ContentHash = prefix.finalize().into();
    if observed_prefix != expected_prefix {
        return Err(io::Error::other(
            "file prefix changed between hashing passes",
        ));
    }
    Ok(full.finalize().into())
}

pub fn hex(hash: &ContentHash) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Owns a temporary source for mutation and replacement tests.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "kartotek-hash-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            for suffix in 0_u64.. {
                let path = base.with_extension(suffix.to_string());
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create fixture: {error}"),
                }
            }
            unreachable!()
        }

        fn observe(&self, bytes: &[u8]) -> FileObservation {
            let path = self.0.join("file");
            fs::write(&path, bytes).unwrap();
            let metadata = FileMetadata::observe(&fs::metadata(&path).unwrap());
            FileObservation { path, metadata }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn empty_file_has_the_standard_sha256_digest() {
        let fixture = Fixture::new();
        let file = fixture.observe(&[]);
        let hash = prefix(&file).unwrap();
        assert_eq!(
            hex(&hash),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(full(&file, hash).unwrap(), hash);
    }

    #[test]
    fn exactly_64_kib_has_the_same_prefix_and_full_digest() {
        let fixture = Fixture::new();
        let file = fixture.observe(&vec![b'x'; PREFIX_BYTES as usize]);
        let hash = prefix(&file).unwrap();
        assert_eq!(full(&file, hash).unwrap(), hash);
    }

    #[test]
    fn full_hash_rejects_an_inconsistent_saved_prefix() {
        let fixture = Fixture::new();
        let file = fixture.observe(&vec![b'x'; PREFIX_BYTES as usize + 1]);
        assert!(full(&file, [0; 32]).is_err());
    }

    #[test]
    fn changes_and_symlink_replacements_are_rejected() {
        let fixture = Fixture::new();
        let file = fixture.observe(b"data");
        fs::write(&file.path, b"longer contents").unwrap();
        assert!(prefix(&file).is_err());
        fs::remove_file(&file.path).unwrap();
        std::os::unix::fs::symlink("missing", &file.path).unwrap();
        assert!(prefix(&file).is_err());
    }

    #[test]
    fn path_replacement_during_an_open_read_is_rejected() {
        let fixture = Fixture::new();
        let observation = fixture.observe(b"data");
        let mut file = open(&observation).unwrap();
        fs::rename(&observation.path, fixture.0.join("original")).unwrap();
        fs::write(&observation.path, b"data").unwrap();
        let mut bytes = [0; 4];
        file.read_exact(&mut bytes).unwrap();
        assert!(validate(&observation, Some(&file)).is_err());
    }
}
