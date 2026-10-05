//! Persists scan observations and legal scan states in an exclusively owned SQLite catalog.

use crate::hashing::{ContentHash, PREFIX_BYTES};
use crate::inventory::{FileMetadata, FileObservation, Observation, ReadFailure, ScanOutcome};
use rusqlite::{Connection, OpenFlags, params};
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

const APPLICATION_ID: i64 = 0x4b41_5254;
const SCHEMA_VERSION: i64 = 2;
const SCHEMA: &str = include_str!("catalog_schema.sql");
const MIGRATE_V2: &str = include_str!("catalog_migrate_v2.sql");

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
    pub hashing: bool,
    pub prefixes: i64,
    pub full_hashes: i64,
}

/// An observed file selected for a hashing pass, with any saved prefix digest.
pub struct HashFile {
    pub observation: FileObservation,
    pub prefix: Option<ContentHash>,
}

/// The two legal stages of candidate hashing.
pub enum HashStage {
    Prefix,
    Full,
}

/// A set of paths with matching full hashes and at least two observed file identities.
pub struct DuplicateGroup {
    pub size: u64,
    pub hash: ContentHash,
    pub files: Vec<(PathBuf, u64, u64)>,
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
        } else if application == APPLICATION_ID && version == 1 {
            transaction
                .execute_batch(MIGRATE_V2)
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

    pub fn start(&self, root: &Path, started_at: i64, hashing: bool) -> io::Result<i64> {
        self.connection
            .execute(
                "INSERT INTO scans (root, state, started_at, hashing) VALUES (?1, 'running', ?2, ?3)",
                params![root.as_os_str().as_bytes(), started_at, hashing],
            )
            .map_err(io::Error::other)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Commits each observation before returning; callback failures preserve earlier observations.
    pub fn record(&self, scan: i64, observation: &Observation) -> io::Result<()> {
        let changed = match observation {
            Observation::File(file) => self.connection.execute(
                "INSERT INTO files (scan_id, path, size, device, inode, links, modified_seconds,
                 modified_nanos, changed_seconds, changed_nanos, mode, uid, gid, hash_state)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 CASE WHEN hashing = 1 THEN 'pending' ELSE 'not_requested' END
                 FROM scans WHERE id = ?1 AND state = 'running'",
                params![scan, file.path.as_os_str().as_bytes(), file.metadata.size.to_string(),
                    file.metadata.device.to_string(), file.metadata.inode.to_string(), file.metadata.links.to_string(),
                    file.metadata.modified_seconds, file.metadata.modified_nanos,
                    file.metadata.changed_seconds, file.metadata.changed_nanos,
                    file.metadata.mode, file.metadata.uid, file.metadata.gid],
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
                 AND (?2 != 'complete' OR (
                     NOT EXISTS (SELECT 1 FROM read_failures WHERE scan_id = ?1)
                     AND NOT EXISTS (SELECT 1 FROM files WHERE scan_id = ?1 AND hash_state IN ('pending', 'failed'))
                     AND NOT EXISTS (
                         SELECT 1 FROM files AS pending JOIN files AS other
                         ON pending.scan_id = other.scan_id AND pending.size = other.size AND pending.prefix_hash = other.prefix_hash
                         WHERE pending.scan_id = ?1 AND pending.hash_state = 'prefix'
                         AND other.hash_state IN ('prefix', 'full')
                         AND (pending.device != other.device OR pending.inode != other.inode))))",
                params![scan, state, time, note],
            )
            .map_err(io::Error::other)?;
        if changed != 1 {
            return Err(io::Error::other(
                "scan must be running; complete scans cannot contain failures or unresolved hashes",
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
             , hashing,
             (SELECT count(*) FROM files WHERE scan_id = scans.id AND hash_state IN ('prefix', 'full')),
             (SELECT count(*) FROM files WHERE scan_id = scans.id AND hash_state = 'full')
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
                hashing: row.get(8).map_err(io::Error::other)?,
                prefixes: row.get(9).map_err(io::Error::other)?,
                full_hashes: row.get(10).map_err(io::Error::other)?,
            });
        }
        Ok(scans)
    }

    pub fn hash_files(&self, scan: i64, stage: HashStage) -> io::Result<Vec<HashFile>> {
        let selection = match stage {
            HashStage::Prefix => "hash_state = 'pending'",
            HashStage::Full => {
                "hash_state = 'prefix' AND (size, prefix_hash) IN (
                SELECT size, prefix_hash FROM files WHERE scan_id = ?1 AND hash_state IN ('prefix', 'full')
                GROUP BY size, prefix_hash HAVING count(DISTINCT device || ':' || inode) > 1)"
            }
        };
        let sql = format!(
            "SELECT path, size, device, inode, links, modified_seconds, modified_nanos,
            changed_seconds, changed_nanos, mode, uid, gid, prefix_hash
            FROM files WHERE scan_id = ?1 AND {selection} ORDER BY path"
        );
        let mut query = self.connection.prepare(&sql).map_err(io::Error::other)?;
        let mut rows = query.query([scan]).map_err(io::Error::other)?;
        let mut files = Vec::new();
        while let Some(row) = rows.next().map_err(io::Error::other)? {
            let path: Vec<u8> = row.get(0).map_err(io::Error::other)?;
            let prefix: Option<Vec<u8>> = row.get(12).map_err(io::Error::other)?;
            let prefix = prefix
                .map(|bytes| {
                    bytes
                        .try_into()
                        .map_err(|_| io::Error::other("invalid saved digest"))
                })
                .transpose()?;
            files.push(HashFile {
                observation: FileObservation {
                    path: PathBuf::from(OsString::from_vec(path)),
                    metadata: FileMetadata {
                        size: row
                            .get::<_, String>(1)
                            .map_err(io::Error::other)?
                            .parse()
                            .map_err(io::Error::other)?,
                        device: row
                            .get::<_, String>(2)
                            .map_err(io::Error::other)?
                            .parse()
                            .map_err(io::Error::other)?,
                        inode: row
                            .get::<_, String>(3)
                            .map_err(io::Error::other)?
                            .parse()
                            .map_err(io::Error::other)?,
                        links: row
                            .get::<_, String>(4)
                            .map_err(io::Error::other)?
                            .parse()
                            .map_err(io::Error::other)?,
                        modified_seconds: row.get(5).map_err(io::Error::other)?,
                        modified_nanos: row.get(6).map_err(io::Error::other)?,
                        changed_seconds: row.get(7).map_err(io::Error::other)?,
                        changed_nanos: row.get(8).map_err(io::Error::other)?,
                        mode: row.get(9).map_err(io::Error::other)?,
                        uid: row.get(10).map_err(io::Error::other)?,
                        gid: row.get(11).map_err(io::Error::other)?,
                    },
                },
                prefix,
            });
        }
        Ok(files)
    }

    pub fn save_prefix(
        &self,
        scan: i64,
        file: &FileObservation,
        hash: ContentHash,
    ) -> io::Result<()> {
        let full = file.metadata.size <= PREFIX_BYTES;
        let changed = self
            .connection
            .execute(
                "UPDATE files SET prefix_hash = ?3, full_hash = ?4, hash_state = ?5
             WHERE scan_id = ?1 AND path = ?2 AND hash_state = 'pending'
             AND EXISTS (SELECT 1 FROM scans WHERE id = ?1 AND state = 'running')",
                params![
                    scan,
                    file.path.as_os_str().as_bytes(),
                    hash.as_slice(),
                    full.then_some(hash.as_slice()),
                    if full { "full" } else { "prefix" }
                ],
            )
            .map_err(io::Error::other)?;
        if changed != 1 {
            return Err(io::Error::other(
                "prefix hashing requires a pending file in a running scan",
            ));
        }
        Ok(())
    }

    pub fn save_full(
        &self,
        scan: i64,
        file: &FileObservation,
        hash: ContentHash,
    ) -> io::Result<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE files SET full_hash = ?3, hash_state = 'full'
             WHERE scan_id = ?1 AND path = ?2 AND hash_state = 'prefix'
             AND EXISTS (SELECT 1 FROM scans WHERE id = ?1 AND state = 'running')",
                params![scan, file.path.as_os_str().as_bytes(), hash.as_slice()],
            )
            .map_err(io::Error::other)?;
        if changed != 1 {
            return Err(io::Error::other(
                "full hashing requires a prefixed file in a running scan",
            ));
        }
        Ok(())
    }

    pub fn hash_failed(&self, scan: i64, failure: ReadFailure) -> io::Result<()> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(io::Error::other)?;
        let changed = transaction
            .execute(
                "UPDATE files SET hash_state = 'failed', full_hash = NULL
             WHERE scan_id = ?1 AND path = ?2 AND hash_state IN ('pending', 'prefix')
             AND EXISTS (SELECT 1 FROM scans WHERE id = ?1 AND state = 'running')",
                params![scan, failure.path.as_os_str().as_bytes()],
            )
            .map_err(io::Error::other)?;
        if changed != 1 {
            return Err(io::Error::other(
                "hash failure requires an unfinished file in a running scan",
            ));
        }
        transaction
            .execute(
                "INSERT INTO read_failures (scan_id, path, kind, message) VALUES (?1, ?2, ?3, ?4)",
                params![
                    scan,
                    failure.path.as_os_str().as_bytes(),
                    format!("{:?}", failure.error.kind()),
                    failure.error.to_string()
                ],
            )
            .map_err(io::Error::other)?;
        transaction.commit().map_err(io::Error::other)
    }

    pub fn duplicates(&self, scan: i64) -> io::Result<Vec<DuplicateGroup>> {
        let hashing: bool = self
            .connection
            .query_row("SELECT hashing FROM scans WHERE id = ?1", [scan], |row| {
                row.get(0)
            })
            .map_err(io::Error::other)?;
        if !hashing {
            return Err(io::Error::other(
                "scan has no requested hashes; run a new scan with --hash",
            ));
        }
        let mut query = self.connection.prepare(
            "SELECT size, full_hash FROM files WHERE scan_id = ?1 AND hash_state = 'full'
             GROUP BY size, full_hash HAVING count(DISTINCT device || ':' || inode) > 1 ORDER BY length(size) DESC, size DESC, full_hash",
        ).map_err(io::Error::other)?;
        let groups: Vec<(String, Vec<u8>)> = query
            .query_map([scan], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(io::Error::other)?
            .collect::<rusqlite::Result<_>>()
            .map_err(io::Error::other)?;
        let mut duplicates = Vec::new();
        for (size, hash) in groups {
            let mut paths = self.connection.prepare(
                "SELECT path, device, inode FROM files WHERE scan_id = ?1 AND size = ?2 AND full_hash = ?3 AND hash_state = 'full' ORDER BY path",
            ).map_err(io::Error::other)?;
            let mut rows = paths
                .query(params![scan, size, hash])
                .map_err(io::Error::other)?;
            let mut files = Vec::new();
            while let Some(row) = rows.next().map_err(io::Error::other)? {
                let path: Vec<u8> = row.get(0).map_err(io::Error::other)?;
                let device = row
                    .get::<_, String>(1)
                    .map_err(io::Error::other)?
                    .parse()
                    .map_err(io::Error::other)?;
                let inode = row
                    .get::<_, String>(2)
                    .map_err(io::Error::other)?
                    .parse()
                    .map_err(io::Error::other)?;
                files.push((PathBuf::from(OsString::from_vec(path)), device, inode));
            }
            duplicates.push(DuplicateGroup {
                size: size.parse().map_err(io::Error::other)?,
                hash: hash
                    .try_into()
                    .map_err(|_| io::Error::other("invalid full digest"))?,
                files,
            });
        }
        Ok(duplicates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{FileObservation, ReadFailure};

    fn metadata(size: u64) -> FileMetadata {
        FileMetadata {
            size,
            device: 1,
            inode: 1,
            links: 1,
            modified_seconds: 0,
            modified_nanos: 0,
            changed_seconds: 0,
            changed_nanos: 0,
            mode: 0,
            uid: 0,
            gid: 0,
        }
    }

    fn catalog() -> Catalog {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        Catalog { connection }
    }

    #[test]
    fn terminal_states_reject_more_observations_and_repeated_finalization() {
        for outcome in [ScanOutcome::Complete, ScanOutcome::Incomplete] {
            let catalog = catalog();
            let id = catalog.start(Path::new("/disk"), 1, false).unwrap();
            catalog.finish(id, outcome, 2).unwrap();
            let observation = Observation::File(FileObservation {
                path: "/disk/file".into(),
                metadata: metadata(1),
            });
            assert!(catalog.record(id, &observation).is_err());
            assert!(catalog.interrupt(id, 3, "late error").is_err());
            assert!(catalog.finish(id, ScanOutcome::Complete, 3).is_err());
        }
    }

    #[test]
    fn maximum_unsigned_size_and_failure_details_are_preserved() {
        let catalog = catalog();
        let id = catalog.start(Path::new("/disk"), 1, false).unwrap();
        catalog
            .record(
                id,
                &Observation::File(FileObservation {
                    path: "/disk/file".into(),
                    metadata: metadata(u64::MAX),
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
        assert!(catalog.start(Path::new("/other"), 3, false).is_ok());
    }

    #[test]
    fn unresolved_candidates_cannot_complete_and_interruption_retains_hashes() {
        let catalog = catalog();
        let id = catalog.start(Path::new("/disk"), 1, true).unwrap();
        let first = FileObservation {
            path: "/disk/first".into(),
            metadata: metadata(PREFIX_BYTES + 1),
        };
        let mut second = FileObservation {
            path: "/disk/second".into(),
            metadata: first.metadata.clone(),
        };
        second.metadata.inode = 2;
        for file in [&first, &second] {
            catalog
                .record(
                    id,
                    &Observation::File(FileObservation {
                        path: file.path.clone(),
                        metadata: file.metadata.clone(),
                    }),
                )
                .unwrap();
        }
        assert!(catalog.finish(id, ScanOutcome::Complete, 2).is_err());
        for file in [&first, &second] {
            catalog.save_prefix(id, file, [1; 32]).unwrap();
        }
        assert!(catalog.finish(id, ScanOutcome::Complete, 2).is_err());
        catalog.save_full(id, &first, [2; 32]).unwrap();
        assert!(catalog.finish(id, ScanOutcome::Complete, 2).is_err());
        catalog
            .interrupt(id, 3, "stopped during full hashing")
            .unwrap();
        let scan = catalog.scans().unwrap().remove(0);
        assert_eq!(scan.state, ScanState::Interrupted);
        assert_eq!((scan.prefixes, scan.full_hashes), (2, 1));
        assert!(catalog.save_full(id, &second, [2; 32]).is_err());
    }

    #[test]
    fn storage_failure_keeps_the_committed_prefix_and_never_marks_complete() {
        let catalog = catalog();
        let id = catalog.start(Path::new("/disk"), 1, false).unwrap();
        let observation = Observation::File(FileObservation {
            path: "/disk/file".into(),
            metadata: metadata(1),
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
