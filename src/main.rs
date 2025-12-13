#![forbid(clippy::allow_attributes)]
#![forbid(clippy::complexity)]
#![forbid(clippy::correctness)]
#![forbid(clippy::expect_used)]
#![forbid(clippy::pedantic)]
#![forbid(clippy::perf)]
#![forbid(clippy::style)]
#![forbid(clippy::suspicious)]
#![forbid(clippy::unwrap_used)]
#![forbid(future_incompatible)]
#![forbid(unsafe_code)]

mod config;
mod parse_notes;
mod utils;

use config::{Config, Mode};

fn main() {
    // Parse command-line arguments into configuration
    let config = match Config::from_args() {
        Ok(config) => config,
        Err(error) => {
            // Print errors to stderr and exit with failure code
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    };

    // Dispatch to the appropriate mode handler
    let result = match config.mode {
        Mode::ParseNotes => parse_notes::run(&config),
        _ => todo!(),
    };

    // Handle any errors from mode execution
    if let Err(error) = result {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}
