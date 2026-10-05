//! Presents read-only directory observations through the command line.

mod inventory;

use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

fn run() -> io::Result<ExitCode> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    if arguments.len() == 1 && (arguments[0] == "--help" || arguments[0] == "-h") {
        writeln!(
            io::stdout().lock(),
            "Usage: kartotek scan <directory>\nLists regular files as bytes followed by a quoted path. Symlinks are skipped."
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if arguments.len() != 2 || arguments[0] != "scan" {
        writeln!(io::stderr().lock(), "Usage: kartotek scan <directory>")?;
        return Ok(ExitCode::from(2));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let outcome = inventory::scan(Path::new(&arguments[1]), |observation| match observation {
        inventory::Observation::File(file) => writeln!(output, "{}\t{:?}", file.size, file.path),
        inventory::Observation::Failure(failure) => {
            writeln!(
                io::stderr().lock(),
                "Cannot read {:?}: {}",
                failure.path,
                failure.error
            )
        }
    })?;
    output.flush()?;
    match outcome {
        inventory::ScanOutcome::Complete => {
            writeln!(io::stderr().lock(), "Scan complete.")?;
            Ok(ExitCode::SUCCESS)
        }
        inventory::ScanOutcome::Incomplete => {
            writeln!(
                io::stderr().lock(),
                "Scan incomplete: some entries could not be observed."
            )?;
            Ok(ExitCode::FAILURE)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "Scan stopped before completion: {error}"
            );
            ExitCode::FAILURE
        }
    }
}
