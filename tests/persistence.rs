//! Exercises persisted CLI outcomes and source/catalog integrity boundaries.

use rusqlite::{Connection, OpenFlags};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// Owns the temporary source tree and catalog used by one integration test.
struct Fixture {
    directory: PathBuf,
    source: PathBuf,
    catalog: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "kartotek-persistence-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let directory = (0_u64..)
            .find_map(|suffix| {
                let candidate = base.with_extension(suffix.to_string());
                match fs::create_dir(&candidate) {
                    Ok(()) => Some(candidate),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("cannot create test directory: {error}"),
                }
            })
            .unwrap();
        let source = directory.join("source");
        fs::create_dir(&source).unwrap();
        let catalog = directory.join("catalog.sqlite");
        Self {
            directory,
            source,
            catalog,
        }
    }

    fn run(&self, catalog: &Path, command: &str, root: Option<&Path>) -> Output {
        let mut process = Command::new(env!("CARGO_BIN_EXE_kartotek"));
        process.arg("--catalog").arg(catalog).arg(command);
        if let Some(root) = root {
            process.arg(root);
        }
        process.output().unwrap()
    }

    fn scan(&self) -> Output {
        self.run(&self.catalog, "scan", Some(&self.source))
    }

    fn hash_scan(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_kartotek"))
            .arg("--catalog")
            .arg(&self.catalog)
            .arg("scan")
            .arg(&self.source)
            .arg("--hash")
            .output()
            .unwrap()
    }

    fn duplicates(&self, scan: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_kartotek"))
            .arg("--catalog")
            .arg(&self.catalog)
            .arg("duplicates")
            .arg(scan)
            .output()
            .unwrap()
    }

    fn read(&self) -> Connection {
        Connection::open_with_flags(&self.catalog, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    fn large_source(&self) {
        // More output than a pipe can hold keeps failure tests from racing completion.
        for index in 0..1000 {
            fs::write(
                self.source.join(format!("{index:04}-{}", "x".repeat(180))),
                b"data",
            )
            .unwrap();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[test]
fn saves_observations_and_lists_scans_when_source_is_offline() {
    let fixture = Fixture::new();
    let name = OsString::from_vec(b"tab\tline\nnonutf8-\xff".to_vec());
    let path = fixture.source.join(&name);
    fs::write(&path, b"contents").unwrap();
    assert!(fixture.scan().status.success());
    assert!(fixture.scan().status.success());
    assert_eq!(fs::read(&path).unwrap(), b"contents");
    let connection = fixture.read();
    let rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM scans WHERE state = 'complete' AND finished_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 2);
    let size: String = connection
        .query_row("SELECT size FROM files WHERE scan_id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(size, "8");
    let saved: Vec<u8> = connection
        .query_row("SELECT path FROM files WHERE scan_id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(PathBuf::from(OsString::from_vec(saved)), path);
    drop(connection);
    fs::rename(&fixture.source, fixture.directory.join("disconnected")).unwrap();
    let list = fixture.run(&fixture.catalog, "scans", None);
    assert!(list.status.success());
    assert!(
        String::from_utf8(list.stdout)
            .unwrap()
            .contains("1\tcomplete\t1\t0\t")
    );
}

#[test]
fn refuses_catalogs_in_the_source_even_through_parent_symlinks() {
    let fixture = Fixture::new();
    let direct = fixture.source.join("catalog.sqlite");
    assert!(
        !fixture
            .run(&direct, "scan", Some(&fixture.source))
            .status
            .success()
    );
    let alias = fixture.directory.join("source-alias");
    symlink(&fixture.source, &alias).unwrap();
    assert!(
        !fixture
            .run(&alias.join("catalog.sqlite"), "scan", Some(&fixture.source))
            .status
            .success()
    );
    assert!(!direct.exists());
    assert!(
        !fixture
            .run(&fixture.catalog, "scan", Some(&alias))
            .status
            .success()
    );
    assert!(!fixture.catalog.exists());
}

#[test]
fn refuses_linked_catalogs_and_linked_sqlite_sidecars() {
    let fixture = Fixture::new();
    let source = fixture.source.join("original");
    fs::write(&source, b"do not modify").unwrap();
    symlink(&source, &fixture.catalog).unwrap();
    assert!(!fixture.scan().status.success());
    fs::remove_file(&fixture.catalog).unwrap();
    fs::hard_link(&source, &fixture.catalog).unwrap();
    assert!(!fixture.scan().status.success());
    fs::remove_file(&fixture.catalog).unwrap();
    let journal = fixture.directory.join("catalog.sqlite-journal");
    symlink(&source, &journal).unwrap();
    assert!(!fixture.scan().status.success());
    assert!(!fixture.catalog.exists());
    assert_eq!(fs::read(source).unwrap(), b"do not modify");
}

#[test]
fn refuses_foreign_databases_and_unknown_schema_versions_without_changes() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.catalog).unwrap();
    connection
        .execute_batch("CREATE TABLE other (value TEXT); INSERT INTO other VALUES ('retain');")
        .unwrap();
    drop(connection);
    let before = fs::read(&fixture.catalog).unwrap();
    assert!(!fixture.scan().status.success());
    assert_eq!(fs::read(&fixture.catalog).unwrap(), before);
    fs::remove_file(&fixture.catalog).unwrap();
    assert!(fixture.scan().status.success());
    let connection = Connection::open(&fixture.catalog).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    let before = fs::read(&fixture.catalog).unwrap();
    assert!(
        !fixture
            .run(&fixture.catalog, "scans", None)
            .status
            .success()
    );
    assert_eq!(fs::read(&fixture.catalog).unwrap(), before);
}

#[test]
fn read_commands_preserve_uninitialized_files_and_databases() {
    for sqlite_header in [false, true] {
        let fixture = Fixture::new();
        fs::write(&fixture.catalog, []).unwrap();
        if sqlite_header {
            Connection::open(&fixture.catalog)
                .unwrap()
                .execute_batch("VACUUM")
                .unwrap();
        }
        let before = fs::read(&fixture.catalog).unwrap();
        for output in [
            fixture.run(&fixture.catalog, "scans", None),
            fixture.duplicates("1"),
        ] {
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("not an initialized"));
            assert_eq!(fs::read(&fixture.catalog).unwrap(), before);
        }
        assert!(fixture.scan().status.success());
    }
}

#[test]
fn listing_a_missing_catalog_does_not_create_it() {
    let fixture = Fixture::new();
    assert!(
        !fixture
            .run(&fixture.catalog, "scans", None)
            .status
            .success()
    );
    assert!(!fixture.catalog.exists());
}

#[test]
fn abandoned_scan_is_recovered_without_fabricating_an_interruption_time() {
    let fixture = Fixture::new();
    assert!(fixture.scan().status.success());
    let connection = Connection::open(&fixture.catalog).unwrap();
    connection
        .execute(
            "INSERT INTO scans (root, state, started_at) VALUES (?1, 'running', 1)",
            [b"/offline".as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO files (scan_id, path, size) VALUES (2, ?1, '18446744073709551615')",
            [b"/offline/file".as_slice()],
        )
        .unwrap();
    drop(connection);
    assert!(
        fixture
            .run(&fixture.catalog, "scans", None)
            .status
            .success()
    );
    let connection = fixture.read();
    let state: String = connection
        .query_row("SELECT state FROM scans WHERE id = 2", [], |row| row.get(0))
        .unwrap();
    let stopped: Option<i64> = connection
        .query_row("SELECT finished_at FROM scans WHERE id = 2", [], |row| {
            row.get(0)
        })
        .unwrap();
    let files: i64 = connection
        .query_row("SELECT count(*) FROM files WHERE scan_id = 2", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(state, "interrupted");
    assert_eq!(stopped, None);
    assert_eq!(files, 1);
}

#[test]
fn active_catalog_cannot_be_recovered_by_a_second_process() {
    let fixture = Fixture::new();
    assert!(fixture.scan().status.success());
    let connection = Connection::open(&fixture.catalog).unwrap();
    connection.execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; INSERT INTO scans (root, state, started_at) VALUES (X'2f', 'running', 1); COMMIT;").unwrap();
    let second = fixture.run(&fixture.catalog, "scans", None);
    assert!(!second.status.success());
    assert!(String::from_utf8(second.stderr).unwrap().contains("locked"));
    let state: String = connection
        .query_row("SELECT state FROM scans WHERE id = 2", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, "running");
}

#[test]
fn killed_process_preserves_committed_files_and_recovers_as_interrupted() {
    let fixture = Fixture::new();
    fixture.large_source();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kartotek"))
        .arg("--catalog")
        .arg(&fixture.catalog)
        .arg("scan")
        .arg(&fixture.source)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    output.read_line(&mut first).unwrap();
    assert!(!first.is_empty());
    let simultaneous = fixture.run(&fixture.catalog, "scans", None);
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    assert!(!simultaneous.status.success());
    // A killed writer may leave a hot journal; SQLite needs write access to roll
    // it back before inspecting the durable prefix. This does not recover scan states.
    let before = Connection::open(&fixture.catalog).unwrap();
    let state: String = before
        .query_row("SELECT state FROM scans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, "running");
    drop(before);
    let list = fixture.run(&fixture.catalog, "scans", None);
    assert!(list.status.success());
    let connection = fixture.read();
    let (state, count, stopped): (String, i64, Option<i64>) = connection
        .query_row(
            "SELECT state, (SELECT count(*) FROM files), finished_at FROM scans",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, "interrupted");
    assert!(count > 0 && count < 1000);
    assert_eq!(stopped, None);
}

#[test]
fn closed_output_preserves_observations_and_records_an_interruption() {
    let fixture = Fixture::new();
    fixture.large_source();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kartotek"))
        .arg("--catalog")
        .arg(&fixture.catalog)
        .arg("scan")
        .arg(&fixture.source)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    assert!(!child.wait().unwrap().success());
    let connection = fixture.read();
    let (state, count, note): (String, i64, Option<String>) = connection
        .query_row(
            "SELECT state, (SELECT count(*) FROM files), note FROM scans",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, "interrupted");
    assert!(count > 0 && count < 1000);
    assert!(note.is_some());
}

#[test]
fn unreadable_directory_saves_failures_and_marks_scan_incomplete() {
    let fixture = Fixture::new();
    let denied = fixture.source.join("denied");
    fs::create_dir(&denied).unwrap();
    fs::write(fixture.source.join("readable"), b"data").unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o0)).unwrap();
    // Root can bypass these permissions; do not misreport that case as a scan failure.
    if fs::read_dir(&denied).is_ok() {
        fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("permission test requires a user without DAC override");
        return;
    }
    let result = fixture.scan();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(result.status.code(), Some(1));
    let connection = fixture.read();
    let state: String = connection
        .query_row("SELECT state FROM scans", [], |row| row.get(0))
        .unwrap();
    let kind: String = connection
        .query_row("SELECT kind FROM read_failures", [], |row| row.get(0))
        .unwrap();
    let files: i64 = connection
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, "incomplete");
    assert_eq!(kind, "PermissionDenied");
    assert_eq!(files, 1);
}

#[test]
fn staged_hashes_separate_matching_prefixes_from_matching_contents() {
    let fixture = Fixture::new();
    let mut contents = vec![b'a'; 65537];
    contents[65536] = b'x';
    fs::write(fixture.source.join("original"), &contents).unwrap();
    fs::write(fixture.source.join("copy"), &contents).unwrap();
    fs::hard_link(
        fixture.source.join("original"),
        fixture.source.join("alias"),
    )
    .unwrap();
    contents[65536] = b'y';
    fs::write(fixture.source.join("same-prefix"), &contents).unwrap();
    contents[0] = b'b';
    fs::write(fixture.source.join("different-prefix"), &contents).unwrap();
    fs::write(fixture.source.join("tiny"), b"abc").unwrap();
    let result = fixture.hash_scan();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let connection = fixture.read();
    let full: i64 = connection
        .query_row(
            "SELECT count(*) FROM files WHERE hash_state = 'full'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let prefix: i64 = connection
        .query_row(
            "SELECT count(*) FROM files WHERE hash_state = 'prefix'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((full, prefix), (5, 1));
    let tiny: Vec<u8> = connection
        .query_row("SELECT full_hash FROM files WHERE size = '3'", [], |row| {
            row.get(0)
        })
        .unwrap();
    let hex: String = tiny.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        hex,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let metadata: i64 = connection.query_row("SELECT count(*) FROM files WHERE device IS NOT NULL AND inode IS NOT NULL AND modified_nanos IS NOT NULL AND uid IS NOT NULL", [], |row| row.get(0)).unwrap();
    assert_eq!(metadata, 6);
    drop(connection);
    fs::rename(&fixture.source, fixture.directory.join("offline")).unwrap();
    let duplicates = fixture.duplicates("1");
    assert!(duplicates.status.success());
    let text = String::from_utf8(duplicates.stdout).unwrap();
    assert!(text.contains("1 groups"));
    assert!(text.contains("3 paths\t2 file identities"));
    assert!(text.contains("original") && text.contains("copy") && text.contains("alias"));
    assert!(!text.contains("same-prefix") && !text.contains("different-prefix"));
}

#[test]
fn hard_link_aliases_alone_do_not_trigger_full_hashing() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("file"), vec![b'x'; 65537]).unwrap();
    fs::hard_link(fixture.source.join("file"), fixture.source.join("alias")).unwrap();
    assert!(fixture.hash_scan().status.success());
    let connection = fixture.read();
    let full: i64 = connection
        .query_row(
            "SELECT count(*) FROM files WHERE full_hash IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(full, 0);
    drop(connection);
    let result = fixture.duplicates("1");
    assert!(result.status.success());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("0 groups")
    );
}

#[test]
fn prefix_read_failures_are_saved_and_never_marked_complete() {
    let fixture = Fixture::new();
    let denied = fixture.source.join("denied");
    fs::write(&denied, b"data").unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o0)).unwrap();
    if fs::File::open(&denied).is_ok() {
        fs::set_permissions(&denied, fs::Permissions::from_mode(0o600)).unwrap();
        return;
    }
    let result = fixture.hash_scan();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(result.status.code(), Some(1));
    let connection = fixture.read();
    let state: String = connection
        .query_row("SELECT state FROM scans", [], |row| row.get(0))
        .unwrap();
    let hashing: String = connection
        .query_row("SELECT hash_state FROM files", [], |row| row.get(0))
        .unwrap();
    let kind: String = connection
        .query_row("SELECT kind FROM read_failures", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        (state.as_str(), hashing.as_str(), kind.as_str()),
        ("incomplete", "failed", "PermissionDenied")
    );
}

#[test]
fn version_one_catalogs_migrate_without_inventing_metadata_or_hashes() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.catalog).unwrap();
    connection
        .execute_batch(include_str!("fixtures/catalog_v1.sql"))
        .unwrap();
    connection.execute_batch("PRAGMA application_id = 1262572116; PRAGMA user_version = 1;
        INSERT INTO scans (root, state, started_at, finished_at) VALUES (X'2f6f66666c696e65', 'complete', 1, 2);
        INSERT INTO files (scan_id, path, size) VALUES (1, X'2f6f66666c696e652f66696c65ff', '18446744073709551615');").unwrap();
    drop(connection);
    assert!(
        fixture
            .run(&fixture.catalog, "scans", None)
            .status
            .success()
    );
    let connection = fixture.read();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let (state, count): (String, i64) = connection
        .query_row(
            "SELECT state, (SELECT count(*) FROM files) FROM scans",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let (inode, hash, size): (Option<String>, Option<Vec<u8>>, String) = connection
        .query_row("SELECT inode, prefix_hash, size FROM files", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(version, 2);
    assert_eq!((state.as_str(), count), ("complete", 1));
    assert!(inode.is_none() && hash.is_none());
    assert_eq!(size, u64::MAX.to_string());
    drop(connection);
    assert!(!fixture.duplicates("1").status.success());
    assert!(fixture.hash_scan().status.success());
    let connection = fixture.read();
    let count: i64 = connection
        .query_row("SELECT count(*) FROM scans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
}
