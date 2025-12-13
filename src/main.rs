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

use config::Config;

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
}
