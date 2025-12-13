//! Release Linear Ticket Update
//!
//! A tool for automatically marking Linear tickets as completed when a GitHub
//! release is published.
//!
//! ## Overview
//!
//! This tool processes GitHub release notes to find associated Pull Requests,
//! extracts Linear ticket IDs from those PRs, and marks them as completed in
//! Linear. It can be used as individual pipeline commands or as a single
//! orchestrated workflow.
//!
//! ## Modes
//!
//! - **parse-notes**: Extract PR numbers from release notes
//! - **extract-tickets**: Find Linear tickets in PRs
//! - **update-tickets**: Mark Linear tickets as completed
//! - **orchestrator**: Run the complete pipeline
//!
//! ## External Dependencies
//!
//! This tool delegates to external commands rather than bundling libraries:
//! - `gh` (GitHub CLI) - for accessing GitHub API
//! - `curl` - for HTTP requests to Linear API
//! - `grep` - for pattern matching
//! - `jq` - for JSON manipulation

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
mod extract_tickets;
mod orchestrator;
mod parse_notes;
mod update_tickets;
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
        Mode::ExtractTickets => extract_tickets::run(&config),
        Mode::Orchestrator => orchestrator::run(&config),
        Mode::ParseNotes => parse_notes::run(&config),
        Mode::UpdateTickets => update_tickets::run(&config),
    };

    // Handle any errors from mode execution
    if let Err(error) = result {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}
