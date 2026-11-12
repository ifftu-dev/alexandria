//! Offline evaluator for the Sentinel measurement corpus. No Tauri, no
//! database: it replays corpus entries through `alexandria_studio::integrity`.
//!
//!   cargo run -p alexandria-studio --example integrity_evaluate -- help
//!
//! Exit status: 0 pass, 1 a certified rate was not met, 2 bad input.

use std::process::ExitCode;

use alexandria_studio::integrity::measurement::cli;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = std::io::stdout().lock();
    match cli::run(&args, &mut stdout) {
        Ok(cli::Outcome::Pass) => ExitCode::SUCCESS,
        Ok(cli::Outcome::Fail) => ExitCode::from(1),
        Err(error) => {
            eprintln!("integrity_evaluate: {error}");
            ExitCode::from(2)
        }
    }
}
