//! Presents read-only directory observations through the command line.

mod catalog;
mod catalog_path;
mod hashing;
mod inventory;

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "Usage: kartotek --catalog <file> scan <directory> [--hash]\n       kartotek --catalog <file> scans\n       kartotek --catalog <file> duplicates <scan-id>\nThe catalog must be outside the scanned directory. Symlinks are skipped.";

/// A validated command shape; filesystem validation happens before catalog writes.
enum Command {
    Scan {
        catalog: PathBuf,
        root: PathBuf,
        hashing: bool,
    },
    Scans {
        catalog: PathBuf,
    },
    Duplicates {
        catalog: PathBuf,
        scan: i64,
    },
    Help,
}

fn parse(arguments: &[OsString]) -> Option<Command> {
    match arguments {
        [help] if help == "--help" || help == "-h" => Some(Command::Help),
        [flag, catalog, command, root] if flag == "--catalog" && command == "scan" => {
            Some(Command::Scan {
                catalog: catalog.into(),
                root: root.into(),
                hashing: false,
            })
        }
        [flag, catalog, command, root, hash]
            if flag == "--catalog" && command == "scan" && hash == "--hash" =>
        {
            Some(Command::Scan {
                catalog: catalog.into(),
                root: root.into(),
                hashing: true,
            })
        }
        [flag, catalog, command, scan] if flag == "--catalog" && command == "duplicates" => {
            let scan: i64 = scan.to_str()?.parse().ok()?;
            if scan <= 0 {
                return None;
            }
            Some(Command::Duplicates {
                catalog: catalog.into(),
                scan,
            })
        }
        [flag, catalog, command] if flag == "--catalog" && command == "scans" => {
            Some(Command::Scans {
                catalog: catalog.into(),
            })
        }
        _ => None,
    }
}

fn now() -> io::Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    i64::try_from(seconds).map_err(io::Error::other)
}

fn run() -> io::Result<ExitCode> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    match parse(&arguments) {
        Some(Command::Help) => {
            writeln!(io::stdout().lock(), "{USAGE}")?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Scan {
            catalog,
            root,
            hashing,
        }) => {
            let root = catalog_path::source_root(&root)?;
            let path = catalog_path::destination(&catalog, Some(&root))?;
            let catalog = catalog::Catalog::open(&path, true)?;
            let id = catalog.start(&root, now()?, hashing)?;
            let result = scan(&root, &catalog, id, &path, hashing);
            if let Err(error) = &result {
                // Keep the primary error visible even if recording the failure also fails.
                let finalized =
                    now().and_then(|time| catalog.interrupt(id, time, &error.to_string()));
                if let Err(recording) = finalized {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "Could not finalize scan {id}: {recording}; it will be recovered on the next catalog open."
                    );
                }
            }
            result
        }
        Some(Command::Scans { catalog }) => {
            let path = catalog_path::destination(&catalog, None)?;
            let catalog = catalog::Catalog::open(&path, false)?;
            let mut output = io::stdout().lock();
            writeln!(
                output,
                "id\tstate\tfiles\tfailures\tstarted\tfinished\troot\tnote\thashing\tprefixes\tfull_hashes"
            )?;
            for scan in catalog.scans()? {
                let finished = scan
                    .finished_at
                    .map_or_else(|| "unknown".to_string(), |time| time.to_string());
                let note = scan.note.unwrap_or_default();
                writeln!(
                    output,
                    "{}\t{}\t{}\t{}\t{}\t{}\t{:?}\t{:?}\t{}\t{}\t{}",
                    scan.id,
                    scan.state.as_str(),
                    scan.files,
                    scan.failures,
                    scan.started_at,
                    finished,
                    scan.root,
                    note,
                    scan.hashing,
                    scan.prefixes,
                    scan.full_hashes
                )?;
            }
            output.flush()?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Duplicates { catalog, scan }) => {
            let path = catalog_path::destination(&catalog, None)?;
            let catalog = catalog::Catalog::open(&path, false)?;
            let summary = catalog
                .scans()?
                .into_iter()
                .find(|saved| saved.id == scan)
                .ok_or_else(|| io::Error::other("scan ID does not exist"))?;
            let groups = catalog.duplicates(scan)?;
            let mut output = io::stdout().lock();
            writeln!(
                output,
                "Scan {scan}: {}. {} groups with matching full SHA-256 hashes and sizes.",
                summary.state.as_str(),
                groups.len()
            )?;
            if summary.state != catalog::ScanState::Complete {
                writeln!(
                    output,
                    "Results cover only the successfully observed and hashed subset."
                )?;
            }
            for group in groups {
                let identities: std::collections::HashSet<_> = group
                    .files
                    .iter()
                    .map(|(_, device, inode)| (*device, *inode))
                    .collect();
                writeln!(
                    output,
                    "{}\t{} bytes\t{} paths\t{} file identities",
                    hashing::hex(&group.hash),
                    group.size,
                    group.files.len(),
                    identities.len()
                )?;
                for (path, device, inode) in group.files {
                    writeln!(output, "  {device}:{inode}\t{path:?}")?;
                }
            }
            output.flush()?;
            Ok(ExitCode::SUCCESS)
        }
        None => {
            writeln!(io::stderr().lock(), "{USAGE}")?;
            Ok(ExitCode::from(2))
        }
    }
}

fn scan(
    root: &Path,
    catalog: &catalog::Catalog,
    id: i64,
    path: &Path,
    hashing: bool,
) -> io::Result<ExitCode> {
    writeln!(io::stderr().lock(), "Saving scan {id} in {path:?}.")?;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut outcome = inventory::scan(root, |observation| {
        catalog.record(id, &observation)?;
        match observation {
            inventory::Observation::File(file) => {
                writeln!(output, "{}\t{:?}", file.metadata.size, file.path)
            }
            inventory::Observation::Failure(failure) => {
                writeln!(
                    io::stderr().lock(),
                    "Cannot read {:?}: {}",
                    failure.path,
                    failure.error
                )
            }
        }
    })?;
    output.flush()?;
    if hashing && hash_scan(catalog, id)? == inventory::ScanOutcome::Incomplete {
        outcome = inventory::ScanOutcome::Incomplete;
    }
    let complete = outcome == inventory::ScanOutcome::Complete;
    catalog.finish(id, outcome, now()?)?;
    // Once persisted, output failures must not rewrite the terminal scan state.
    let (message, code) = if complete {
        ("Scan complete.", ExitCode::SUCCESS)
    } else {
        (
            "Scan incomplete: some entries could not be observed.",
            ExitCode::FAILURE,
        )
    };
    let _ = writeln!(io::stderr().lock(), "{message}");
    Ok(code)
}

fn hash_scan(catalog: &catalog::Catalog, id: i64) -> io::Result<inventory::ScanOutcome> {
    let mut outcome = inventory::ScanOutcome::Complete;
    for (stage, name) in [
        (catalog::HashStage::Prefix, "prefix"),
        (catalog::HashStage::Full, "full-file"),
    ] {
        let files = catalog.hash_files(id, stage)?;
        let count = files.len();
        let bytes: u128 = files
            .iter()
            .map(|file| {
                u128::from(if file.prefix.is_some() {
                    file.observation.metadata.size
                } else {
                    file.observation.metadata.size.min(hashing::PREFIX_BYTES)
                })
            })
            .sum();
        writeln!(
            io::stderr().lock(),
            "Scan {id}: {name} hashing {count} files ({bytes} bytes to read)."
        )?;
        for (index, file) in files.into_iter().enumerate() {
            if file.prefix.is_some() {
                writeln!(
                    io::stderr().lock(),
                    "Scan {id}: full-file hashing {}/{count} ({} bytes).",
                    index + 1,
                    file.observation.metadata.size
                )?;
            }
            let result = match file.prefix {
                Some(prefix) => hashing::full(&file.observation, prefix),
                None => hashing::prefix(&file.observation),
            };
            match result {
                Ok(hash) => {
                    if file.prefix.is_some() {
                        catalog.save_full(id, &file.observation, hash)?;
                    } else {
                        catalog.save_prefix(id, &file.observation, hash)?;
                    }
                }
                Err(error) => {
                    let message = format!("{name} hashing: {error}");
                    catalog.hash_failed(
                        id,
                        inventory::ReadFailure {
                            path: file.observation.path.clone(),
                            error: io::Error::new(error.kind(), message.clone()),
                        },
                    )?;
                    outcome = inventory::ScanOutcome::Incomplete;
                    writeln!(
                        io::stderr().lock(),
                        "Cannot hash {:?}: {message}",
                        file.observation.path
                    )?;
                }
            }
            if (index + 1) % 500 == 0 || index + 1 == count || file.prefix.is_some() {
                writeln!(
                    io::stderr().lock(),
                    "Scan {id}: {name} hashes {}/{count} processed.",
                    index + 1
                )?;
            }
        }
    }
    Ok(outcome)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "Kartotek failed: {error}");
            ExitCode::FAILURE
        }
    }
}
