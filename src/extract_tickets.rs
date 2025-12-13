//! Extract Linear ticket IDs from GitHub Pull Requests.
//!
//! This module implements the `extract-tickets` mode which searches through PR
//! content to find Linear ticket references. It examines:
//! - PR title
//! - PR body
//! - PR comments
//! - Commit message headlines
//! - Commit message bodies
//!
//! Supported ticket formats:
//! - Ticket ID: `ABC-123`
//! - Full URL: `https://linear.app/org/issue/ABC-123`

use std::collections::HashSet;
use std::process::Command;

use crate::config::Config;
use crate::utils;

const NAME: &str = "extract-tickets";
const TICKET_PATTERN: &str = r"[A-Z]{3}-[0-9]+";

macro_rules! log {
    ($fmt:expr) => {
        utils::log(NAME, format_args!($fmt));
    };
}

/// Runs the extract-tickets mode to find Linear tickets in PRs.
///
/// # Input
/// Reads PR numbers from input sources (stdin or files), one per line.
///
/// # Output
/// Prints Linear ticket IDs to stdout, one per line, deduplicated.
/// Outputs immediately in order of discovery (no sorting or buffering).
///
/// # Process
/// For each PR number:
/// 1. Fetch PR data from GitHub (title, body, comments, commits)
/// 2. Search all text content for Linear ticket references
/// 3. Deduplicate and output
///
/// # Errors
/// Returns an error if:
/// - Input sources cannot be read
/// - GitHub CLI fails to fetch PR data
/// - PR number is invalid or inaccessible
pub fn run(config: &Config) -> Result<(), String> {
    // Track seen ticket IDs to avoid duplicates across all PRs
    let mut seen_tickets = HashSet::new();
    let mut any_output = false;

    // Process PR numbers as they arrive from input (streaming).
    log!("reading PR numbers from input");
    utils::for_each_input_line(&config.input_sources, |line| {
        let pr_num = line.trim();
        if pr_num.is_empty() {
            return Ok(());
        }

        log!("processing PR #{pr_num}");

        // Fetch all text content from the PR
        let pr_text = get_pr_text(pr_num)?;

        // Find and output Linear ticket IDs immediately
        find_and_output_tickets(&pr_text, &mut seen_tickets, &mut any_output)?;

        Ok(())
    })?;

    log!("done");
    if !any_output {
        log!("no changes made");
    }

    Ok(())
}

/// Fetches all relevant text content from a GitHub PR.
///
/// # Arguments
/// * `pr_num` - The pull request number to fetch
///
/// # Returns
/// A string containing all searchable text from the PR, with sections
/// separated by double newlines.
///
/// # Text Sources
/// - PR title
/// - PR body
/// - All comment bodies
/// - All commit message headlines
/// - All commit message bodies
///
/// # Errors
/// Returns an error if:
/// - The `gh` command fails to execute
/// - The PR doesn't exist or is inaccessible
/// - The response contains invalid UTF-8
/// - JSON parsing fails
///
/// # Implementation
/// Uses `gh pr view <num> --json` to fetch structured data, then
/// uses `jq` to extract text fields.
fn get_pr_text(pr_num: &str) -> Result<String, String> {
    // Fetch PR data as JSON
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            pr_num,
            "--json",
            "title,body,comments,commits",
        ])
        .output()
        .map_err(|e| format!("Failed to execute gh command: {e}"))?;

    if !output.status.success() {
        return Err(format!("Failed to get PR #{pr_num}"));
    }

    let json_output = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 from gh: {e}"))?;

    macro_rules! jq {
        ($query:literal) => {
            utils::run_jq(&json_output, $query)?
        };
    }

    // Extract text fields from JSON using jq
    // Each field may return empty string if not present
    let text_parts = [
        jq!(r#".title // """#),
        jq!(r#".body // """#),
        jq!(".comments[]?.body // empty"),
        jq!(".commits[]?.messageHeadline // empty"),
        jq!(".commits[]?.messageBody // empty"),
    ];

    // Combine all text parts with double newlines for separation
    Ok(text_parts.join("\n\n"))
}

/// Finds Linear ticket IDs in text and outputs them immediately.
///
/// # Arguments
/// * `text` - The text to search for ticket references
/// * `seen` - `HashSet` to track already-output tickets (prevents duplicates)
///
/// # Output
/// Prints each unique ticket ID to stdout as soon as it's discovered.
/// No sorting or buffering - outputs in order of discovery.
///
/// # Supported Formats
/// - Ticket ID: `ABC-123`
/// - Full URL: `https://linear.app/org/issue/ABC-123` (ID extracted from URL)
///
/// # Pattern
/// Ticket IDs must match the pattern: 3 uppercase ASCII letters, hyphen, one or
/// more digits. Examples: `HIP-123`, `ENG-42`, `BUG-007`
///
/// # Implementation
/// 1. Uses grep to find all ticket ID matches anywhere in the text
/// 2. Outputs each new ticket ID immediately (checks `HashSet`)
fn find_and_output_tickets(
    text: &str,
    seen: &mut HashSet<String>,
    any_output: &mut bool,
) -> Result<(), String> {
    let id_matches = utils::run_grep(text, TICKET_PATTERN)?;
    id_matches
        .lines()
        .filter_map(|id| {
            let trimmed = id.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .for_each(|id| {
            if seen.insert(id.to_string()) {
                println!("{id}");
                *any_output = true;
            }
        });

    Ok(())
}
