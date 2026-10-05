//! Presents read-only directory observations through the command line.

mod catalog;
mod catalog_path;
mod inventory;

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "Usage: kartotek --catalog <file> scan <directory>\n       kartotek --catalog <file> scans\nThe catalog must be outside the scanned directory. Symlinks are skipped.";

/// A validated command shape; filesystem validation happens before catalog writes.
enum Command {
    Scan { catalog: PathBuf, root: PathBuf },
    Scans { catalog: PathBuf },
    Help,
}

fn parse(arguments: &[OsString]) -> Option<Command> {
    match arguments {
        [help] if help == "--help" || help == "-h" => Some(Command::Help),
        [flag, catalog, command, root] if flag == "--catalog" && command == "scan" => {
            Some(Command::Scan {
                catalog: catalog.into(),
                root: root.into(),
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
        Some(Command::Scan { catalog, root }) => {
            let root = catalog_path::source_root(&root)?;
            let path = catalog_path::destination(&catalog, Some(&root))?;
            let catalog = catalog::Catalog::open(&path, true)?;
            let id = catalog.start(&root, now()?)?;
            let result = scan(&root, &catalog, id, &path);
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
                "id\tstate\tfiles\tfailures\tstarted\tfinished\troot\tnote"
            )?;
            for scan in catalog.scans()? {
                let finished = scan
                    .finished_at
                    .map_or_else(|| "unknown".to_string(), |time| time.to_string());
                let note = scan.note.unwrap_or_default();
                writeln!(
                    output,
                    "{}\t{}\t{}\t{}\t{}\t{}\t{:?}\t{:?}",
                    scan.id,
                    scan.state.as_str(),
                    scan.files,
                    scan.failures,
                    scan.started_at,
                    finished,
                    scan.root,
                    note
                )?;
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

fn scan(root: &Path, catalog: &catalog::Catalog, id: i64, path: &Path) -> io::Result<ExitCode> {
    writeln!(io::stderr().lock(), "Saving scan {id} in {path:?}.")?;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let outcome = inventory::scan(root, |observation| {
        catalog.record(id, &observation)?;
        match observation {
            inventory::Observation::File(file) => {
                writeln!(output, "{}\t{:?}", file.size, file.path)
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

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "Kartotek failed: {error}");
            ExitCode::FAILURE
        }
    }
}
