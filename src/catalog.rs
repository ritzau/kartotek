//! Persists scan observations and legal scan states in an exclusively owned SQLite catalog.

use crate::inventory::{Observation, ScanOutcome};
use rusqlite::{Connection, OpenFlags, params};
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

const APPLICATION_ID: i64 = 0x4b41_5254;
const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = include_str!("catalog_schema.sql");

/// The persisted lifecycle of a scan; terminal states cannot be resumed or rewritten.
#[derive(Debug, PartialEq, Eq)]
pub enum ScanState {
    Running,
    Complete,
    Incomplete,
    Interrupted,
}

impl ScanState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Incomplete => "incomplete",
            Self::Interrupted => "interrupted",
        }
    }
}

/// A saved scan summary, including uncertainty about when an abandoned scan stopped.
#[derive(Debug)]
pub struct ScanSummary {
    pub id: i64,
    pub root: PathBuf,
    pub state: ScanState,
    pub files: i64,
    pub failures: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub note: Option<String>,
}

/// Owns one synchronous connection; exclusive locking prevents concurrent use and unsafe recovery.
pub struct Catalog {
    connection: Connection,
}

impl Catalog {
    pub fn open(path: &Path, create: bool) -> io::Result<Self> {
        // Do not interpret user paths as SQLite URIs or enable shared connections.
        let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        if create {
            flags |= OpenFlags::SQLITE_OPEN_CREATE;
        }
        let mut connection = Connection::open_with_flags(path, flags).map_err(io::Error::other)?;
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(io::Error::other)?;
        connection
            .execute_batch(
                "PRAGMA locking_mode = EXCLUSIVE;
                 PRAGMA foreign_keys = ON;
                 PRAGMA synchronous = FULL;",
            )
            .map_err(io::Error::other)?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive)
            .map_err(io::Error::other)?;
        let application: i64 = transaction
            .query_row("PRAGMA application_id", [], |row| row.get(0))
            .map_err(io::Error::other)?;
        let version: i64 = transaction
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(io::Error::other)?;
        let journal: String = transaction
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(io::Error::other)?;
        if journal != "delete" {
            return Err(io::Error::other(
                "catalog requires SQLite DELETE journal mode",
            ));
        }
        if application == 0 && version == 0 {
            let objects: i64 = transaction
                .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
                .map_err(io::Error::other)?;
            if objects != 0 {
                return Err(io::Error::other("file is not a Kartotek catalog"));
            }
            transaction
                .execute_batch(SCHEMA)
                .map_err(io::Error::other)?;
            transaction
                .pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(io::Error::other)?;
            transaction
                .pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(io::Error::other)?;
        } else if application != APPLICATION_ID || version != SCHEMA_VERSION {
            return Err(io::Error::other(
                "unrecognized catalog or unsupported schema version",
            ));
        }
        // The exclusive lock remains held across commits until this connection closes.
        // No other Kartotek process can still own these running scans.
        transaction
            .execute(
                "UPDATE scans SET state = 'interrupted',
                 note = 'previous process stopped before completion; interruption time unknown'
                 WHERE state = 'running'",
                [],
            )
            .map_err(io::Error::other)?;
        transaction.commit().map_err(io::Error::other)?;
        Ok(Self { connection })
    }

    pub fn start(&self, root: &Path, started_at: i64) -> io::Result<i64> {
        self.connection
            .execute(
                "INSERT INTO scans (root, state, started_at) VALUES (?1, 'running', ?2)",
                params![root.as_os_str().as_bytes(), started_at],
            )
            .map_err(io::Error::other)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Commits each observation before returning; callback failures preserve earlier observations.
    pub fn record(&self, scan: i64, observation: &Observation) -> io::Result<()> {
        let changed = match observation {
            Observation::File(file) => self.connection.execute(
                "INSERT INTO files (scan_id, path, size)
                 SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM scans WHERE id = ?1 AND state = 'running')",
                params![scan, file.path.as_os_str().as_bytes(), file.size.to_string()],
            ),
            Observation::Failure(failure) => self.connection.execute(
                "INSERT INTO read_failures (scan_id, path, kind, message)
                 SELECT ?1, ?2, ?3, ?4 WHERE EXISTS (SELECT 1 FROM scans WHERE id = ?1 AND state = 'running')",
                params![scan, failure.path.as_os_str().as_bytes(), format!("{:?}", failure.error.kind()), failure.error.to_string()],
            ),
        }
        .map_err(io::Error::other)?;
        if changed != 1 {
            return Err(io::Error::other("observations require a running scan"));
        }
        Ok(())
    }

    pub fn finish(&self, scan: i64, outcome: ScanOutcome, finished_at: i64) -> io::Result<()> {
        let state = match outcome {
            ScanOutcome::Complete => "complete",
            ScanOutcome::Incomplete => "incomplete",
        };
        self.finalize(scan, state, finished_at, None)
    }

    pub fn interrupt(&self, scan: i64, stopped_at: i64, reason: &str) -> io::Result<()> {
        self.finalize(scan, "interrupted", stopped_at, Some(reason))
    }

    fn finalize(&self, scan: i64, state: &str, time: i64, note: Option<&str>) -> io::Result<()> {
        let changed = self.connection
            .execute(
                "UPDATE scans SET state = ?2, finished_at = ?3, note = ?4
                 WHERE id = ?1 AND state = 'running'
                 AND (?2 != 'complete' OR NOT EXISTS (SELECT 1 FROM read_failures WHERE scan_id = ?1))",
                params![scan, state, time, note],
            )
            .map_err(io::Error::other)?;
        if changed != 1 {
            return Err(io::Error::other(
                "scan must be running; complete scans cannot contain read failures",
            ));
        }
        Ok(())
    }

    pub fn scans(&self) -> io::Result<Vec<ScanSummary>> {
        let mut query = self
            .connection
            .prepare(
                "SELECT id, root, state, started_at, finished_at, note,
             (SELECT count(*) FROM files WHERE scan_id = scans.id),
             (SELECT count(*) FROM read_failures WHERE scan_id = scans.id)
             FROM scans ORDER BY id",
            )
            .map_err(io::Error::other)?;
        let mut rows = query.query([]).map_err(io::Error::other)?;
        let mut scans = Vec::new();
        while let Some(row) = rows.next().map_err(io::Error::other)? {
            let state: String = row.get(2).map_err(io::Error::other)?;
            let state = match state.as_str() {
                "running" => ScanState::Running,
                "complete" => ScanState::Complete,
                "incomplete" => ScanState::Incomplete,
                "interrupted" => ScanState::Interrupted,
                _ => return Err(io::Error::other("invalid saved scan state")),
            };
            let root: Vec<u8> = row.get(1).map_err(io::Error::other)?;
            scans.push(ScanSummary {
                id: row.get(0).map_err(io::Error::other)?,
                root: PathBuf::from(OsString::from_vec(root)),
                state,
                files: row.get(6).map_err(io::Error::other)?,
                failures: row.get(7).map_err(io::Error::other)?,
                started_at: row.get(3).map_err(io::Error::other)?,
                finished_at: row.get(4).map_err(io::Error::other)?,
                note: row.get(5).map_err(io::Error::other)?,
            });
        }
        Ok(scans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{FileObservation, ReadFailure};

    fn catalog() -> Catalog {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        Catalog { connection }
    }

    #[test]
    fn terminal_states_reject_more_observations_and_repeated_finalization() {
        for outcome in [ScanOutcome::Complete, ScanOutcome::Incomplete] {
            let catalog = catalog();
            let id = catalog.start(Path::new("/disk"), 1).unwrap();
            catalog.finish(id, outcome, 2).unwrap();
            let observation = Observation::File(FileObservation {
                path: "/disk/file".into(),
                size: 1,
            });
            assert!(catalog.record(id, &observation).is_err());
            assert!(catalog.interrupt(id, 3, "late error").is_err());
            assert!(catalog.finish(id, ScanOutcome::Complete, 3).is_err());
        }
    }

    #[test]
    fn maximum_unsigned_size_and_failure_details_are_preserved() {
        let catalog = catalog();
        let id = catalog.start(Path::new("/disk"), 1).unwrap();
        catalog
            .record(
                id,
                &Observation::File(FileObservation {
                    path: "/disk/file".into(),
                    size: u64::MAX,
                }),
            )
            .unwrap();
        catalog
            .record(
                id,
                &Observation::Failure(ReadFailure {
                    path: "/disk/missing".into(),
                    error: io::Error::new(io::ErrorKind::NotFound, "disappeared"),
                }),
            )
            .unwrap();
        let size: String = catalog
            .connection
            .query_row("SELECT size FROM files", [], |row| row.get(0))
            .unwrap();
        let (kind, message): (String, String) = catalog
            .connection
            .query_row("SELECT kind, message FROM read_failures", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(size, u64::MAX.to_string());
        assert_eq!(
            (kind.as_str(), message.as_str()),
            ("NotFound", "disappeared")
        );
        assert!(catalog.finish(id, ScanOutcome::Complete, 2).is_err());
        catalog.interrupt(id, 2, "output failed").unwrap();
        let scan = catalog.scans().unwrap().remove(0);
        assert_eq!(scan.state, ScanState::Interrupted);
        assert_eq!(
            (scan.files, scan.failures, scan.finished_at),
            (1, 1, Some(2))
        );
        assert!(catalog.start(Path::new("/other"), 3).is_ok());
    }

    #[test]
    fn storage_failure_keeps_the_committed_prefix_and_never_marks_complete() {
        let catalog = catalog();
        let id = catalog.start(Path::new("/disk"), 1).unwrap();
        let observation = Observation::File(FileObservation {
            path: "/disk/file".into(),
            size: 1,
        });
        catalog.record(id, &observation).unwrap();
        catalog
            .connection
            .execute_batch("PRAGMA query_only = ON")
            .unwrap();
        assert!(catalog.record(id, &observation).is_err());
        assert!(catalog.finish(id, ScanOutcome::Complete, 2).is_err());
        assert!(catalog.interrupt(id, 2, "storage unavailable").is_err());
        let scan = catalog.scans().unwrap().remove(0);
        assert_eq!(scan.state, ScanState::Running);
        assert_eq!(scan.files, 1);
        assert_eq!(scan.finished_at, None);
    }
}
