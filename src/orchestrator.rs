//! Orchestrator mode - chaining all pipeline stages together.
//!
//! This module implements the orchestrator mode which runs the complete
//! workflow by spawning subprocesses for each stage and piping their
//! output together.
//!
//! ## Pipeline
//! ```text
//! parse-notes --release-tag TAG
//!     | (stdout)
//!     v
//! extract-tickets
//!     | (stdout)
//!     v
//! update-tickets --linear-api-key KEY --linear-org ORG [--dry-run]
//!     | (stdout/stderr)
//!     v
//! Output to parent process
//! ```
//!
//! ## Implementation
//! Instead of calling internal functions, this mode spawns the binary
//! as separate processes. This ensures consistency with standalone usage
//! and makes the pipeline composable.

use std::env;
use std::process::{Command, Stdio};

use crate::config::Config;

/// Runs the orchestrator mode to execute the complete pipeline.
///
/// # Required Configuration
/// - `config.release_tag` - The GitHub release tag to process
/// - `LINEAR_API_KEY` (from config or environment)
/// - `LINEAR_ORG` (from config or environment)
///
/// # Pipeline Stages
/// 1. **parse-notes**: Extracts PR numbers from release notes
/// 2. **extract-tickets**: Finds Linear tickets in those PRs
/// 3. **update-tickets**: Marks tickets as completed
///
/// # Process Flow
/// 1. Spawns `parse-notes` subprocess with release tag
/// 2. Spawns `extract-tickets` subprocess, piping from parse-notes
/// 3. Spawns `update-tickets` subprocess, piping from extract-tickets
/// 4. Waits for completion and forwards output to parent
///
/// # Dry-Run Support
/// If `config.dry_run` is true, passes `--dry-run` flag to update-tickets
/// stage.
///
/// # Workflow State Filtering
/// By default, tickets are only updated if their current state name is
/// "Passing". If `config.update_all_statuses` is true, passes
/// `--update-all-statuses` to update-tickets.
///
/// # Output
/// - Forwards stdout from update-tickets to parent stdout
/// - Forwards stderr from update-tickets to parent stderr
///
/// # Errors
/// Returns an error if:
/// - `--release-tag` is not provided
/// - `LINEAR_API_KEY` or `LINEAR_ORG` cannot be determined
/// - Any subprocess fails to spawn
/// - Pipe redirection fails
/// - The pipeline exits with non-zero status
pub fn run(config: &Config) -> Result<(), String> {
    // Validate required configuration
    let release_tag = config.release_tag.as_ref().ok_or_else(|| {
        "Orchestrator mode requires --release-tag flag".to_string()
    })?;

    // Get Linear credentials from config or environment
    let linear_api_key = config.get_linear_api_key()?;
    let linear_org = config.get_linear_org()?;

    // Get path to current executable for spawning subprocesses
    let exe_path = env::current_exe()
        .map_err(|e| format!("Failed to get current executable path: {e}"))?;

    // Stage 1: Parse release notes to extract PR numbers
    // Spawns: release-linear-ticket-update parse-notes --release-tag <TAG>
    let mut parse_cmd = Command::new(&exe_path)
        .args(["parse-notes", "--release-tag", release_tag])
        .stdout(Stdio::piped()) // Capture stdout for piping to next stage
        .spawn()
        .map_err(|e| format!("Failed to spawn parse-notes: {e}"))?;

    let parse_stdout = parse_cmd
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture parse-notes stdout".to_string())?;

    // Stage 2: Extract Linear tickets from PRs
    // Spawns: release-linear-ticket-update extract-tickets
    // Reads from parse-notes stdout
    let mut extract_cmd = Command::new(&exe_path)
        .arg("extract-tickets")
        .stdin(parse_stdout)
        .stdout(Stdio::piped()) // Capture stdout for piping to next stage
        .spawn()
        .map_err(|e| format!("Failed to spawn extract-tickets: {e}"))?;

    let extract_stdout = extract_cmd.stdout.take().ok_or_else(|| {
        "Failed to capture extract-tickets stdout".to_string()
    })?;

    // Stage 3: Update Linear tickets to completed state
    // Build arguments dynamically to include --dry-run if needed
    let mut update_args = vec![
        "update-tickets",
        "--linear-api-key",
        &linear_api_key,
        "--linear-org",
        &linear_org,
    ];

    // Add --dry-run flag if in preview mode
    if config.dry_run {
        update_args.push("--dry-run");
    }

    if config.update_all_statuses {
        update_args.push("--update-all-statuses");
    }

    // Spawns: release-linear-ticket-update update-tickets --linear-api-key
    // <KEY> --linear-org <ORG> [--dry-run] Reads from extract-tickets
    // stdout
    let mut update_child = Command::new(&exe_path)
        .args(&update_args)
        .stdin(extract_stdout)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to spawn update-tickets: {e}"))?;

    // Important: wait on *every* stage so failures don't get masked by a
    // successful last stage.
    let update_status = update_child
        .wait()
        .map_err(|e| format!("Failed to wait for update-tickets: {e}"))?;

    let extract_status = extract_cmd
        .wait()
        .map_err(|e| format!("Failed to wait for extract-tickets: {e}"))?;

    let parse_status = parse_cmd
        .wait()
        .map_err(|e| format!("Failed to wait for parse-notes: {e}"))?;

    if !parse_status.success()
        || !extract_status.success()
        || !update_status.success()
    {
        return Err("Pipeline failed".to_string());
    }

    Ok(())
}
