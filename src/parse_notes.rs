//! Parse release notes to extract Pull Request numbers.
//!
//! This module implements the `parse-notes` mode which extracts PR numbers from
//! GitHub release notes. It supports two input patterns:
//! - Short format: `#123`
//! - Full URL format: `https://github.com/owner/repo/pull/123`
//!
//! The output is deduplicated PR numbers (one per line), printed immediately
//! as they are discovered. No sorting or buffering to minimize latency.

use std::collections::HashSet;
use std::io::{self, BufRead};
use std::process::{Command, Stdio};

use crate::config::Config;
use crate::utils;

const NAME: &str = "parse-notes";

macro_rules! log {
    ($fmt:expr) => {
        utils::log(NAME, format_args!($fmt));
    };
}

/// Runs the parse-notes mode to extract PR numbers from release notes.
///
/// # Input Sources
/// - If `config.release_tag` is set, fetches release notes from GitHub using
///   `gh` CLI
/// - Otherwise, reads release notes from stdin
///
/// # Output
/// Prints PR numbers to stdout, one per line, deduplicated.
/// Outputs immediately in order of discovery (no sorting or buffering).
///
/// # Errors
/// Returns an error if:
/// - GitHub CLI fails to fetch release notes
/// - Release notes contain invalid UTF-8
/// - stdin cannot be read
pub fn run(config: &Config) -> Result<(), String> {
    let mut seen = HashSet::new();
    let any_output = if let Some(ref tag) = config.release_tag {
        log!("streaming release notes for tag {tag}");
        stream_pr_numbers_from_release(tag, &mut seen)?
    } else {
        log!("streaming release notes from stdin");
        stream_pr_numbers_from_stdin(&mut seen)?
    };
    if any_output {
        log!("done");
    } else {
        log!("done");
        log!("no changes made");
    }

    Ok(())
}

fn stream_pr_numbers_from_release(
    tag: &str,
    seen: &mut HashSet<String>,
) -> Result<bool, String> {
    // We stream `gh` output into `grep` so this stage can start emitting PR
    // numbers immediately.
    let mut gh_child = Command::new("gh")
        .args(["release", "view", tag, "--json", "body", "--jq", ".body"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to execute gh command: {e}"))?;

    let gh_stdout = gh_child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture gh stdout".to_string())?;

    let any_output =
        stream_pr_numbers_from_reader(Stdio::from(gh_stdout), seen)?;
    let status = gh_child
        .wait()
        .map_err(|e| format!("Failed to wait for gh: {e}"))?;
    if !status.success() {
        return Err(format!("Failed to get release notes for tag {tag}"));
    }
    Ok(any_output)
}

fn stream_pr_numbers_from_stdin(
    seen: &mut HashSet<String>,
) -> Result<bool, String> {
    stream_pr_numbers_from_reader(Stdio::inherit(), seen)
}

fn stream_pr_numbers_from_reader(
    stdin: Stdio,
    seen: &mut HashSet<String>,
) -> Result<bool, String> {
    // Single pass over the input, emitting matches in discovery order.
    // We let grep do the heavy lifting for matching, then we normalize output
    // to raw PR numbers.
    let pattern = r"(#[0-9]+|https://github\.com/[^/]+/[^/]+/pull/[0-9]+)";

    let mut grep_child = Command::new("grep")
        .args(["-oE", pattern])
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to spawn grep: {e}"))?;

    let grep_stdout = grep_child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture grep stdout".to_string())?;

    let mut any_output = false;
    let reader = io::BufReader::new(grep_stdout);
    for line_result in reader.lines() {
        let matched = line_result
            .map_err(|e| format!("Failed to read grep output: {e}"))?;
        // `grep -oE` returns the matched substring; normalize it to a PR number
        // and dedupe.
        if let Some(num) = matched.strip_prefix('#') {
            if seen.insert(num.to_string()) {
                println!("{num}");
                any_output = true;
            }
            continue;
        }
        if let Some(num) = matched.rsplit('/').next()
            && seen.insert(num.to_string())
        {
            println!("{num}");
            any_output = true;
        }
    }

    let status = grep_child
        .wait()
        .map_err(|e| format!("Failed to wait for grep: {e}"))?;
    if status.success() || status.code() == Some(1) {
        return Ok(any_output);
    }

    Err("grep failed".to_string())
}
