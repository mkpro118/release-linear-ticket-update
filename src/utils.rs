//! Utility functions for external command execution and I/O.
//!
//! This module provides wrappers around external commands (grep, jq, curl)
//! and I/O operations (stdin, file reading). By delegating to system commands
//! rather than bundling libraries, the binary remains small and leverages
//! well-tested Unix tools.
//!
//! ## External Dependencies
//! - `grep` - Pattern matching with regex support
//! - `jq` - JSON parsing and manipulation
//! - `curl` - HTTP requests to Linear GraphQL API

use std::fmt;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use crate::config::InputSource;

// Keep prefixes aligned in stderr output:
//
// parse-notes     :
// extract-tickets :
// update-tickets  :
const LOG_PREFIX_WIDTH: usize = 16;

pub fn log(prefix: &str, message: fmt::Arguments<'_>) {
    // Intentionally hard-coded width for stable, greppable logs.
    eprintln!("{prefix:<LOG_PREFIX_WIDTH$}: {message}");
}

/// Calls a function once per input line across multiple sources.
///
/// # Behavior
/// - Processes sources in the order provided
/// - Stdin can be mixed with files (e.g., `[File("a.txt"), Stdin,
///   File("b.txt")]`)
/// - For stdin, processes lines as they arrive (streaming; does not wait for
///   EOF before starting)
pub fn for_each_input_line<F>(
    sources: &[InputSource],
    mut on_line: F,
) -> Result<(), String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    for source in sources {
        match source {
            InputSource::Stdin => {
                // Streaming by design: we process each line as it arrives,
                // which keeps pipelines flowing (downstream
                // commands don't have to wait for EOF).
                let stdin = io::stdin();
                for line_result in stdin.lock().lines() {
                    let line = line_result.map_err(|e| {
                        format!("Failed to read from stdin: {e}")
                    })?;
                    on_line(&line)?;
                }
            }
            InputSource::File(path) => {
                // Files are processed line-by-line for consistent behavior with
                // stdin.
                let file = std::fs::File::open(path)
                    .map_err(|e| format!("Failed to open file {path}: {e}"))?;
                let reader = io::BufReader::new(file);
                for line_result in reader.lines() {
                    let line = line_result.map_err(|e| {
                        format!("Failed to read file {path}: {e}")
                    })?;
                    on_line(&line)?;
                }
            }
        }
    }
    Ok(())
}

/// Runs grep to find pattern matches in text.
///
/// # Arguments
/// * `input` - The text to search
/// * `pattern` - Extended regex pattern (grep -E syntax)
///
/// # Returns
/// Newline-separated list of matches. Returns empty string if no matches.
///
/// # Options
/// - `-o`: Only output matching parts (not full lines)
/// - `-E`: Extended regex syntax
/// - stderr redirected to null (errors suppressed)
///
/// # Errors
/// Returns an error if:
/// - `grep` command cannot be spawned
/// - Failed to write input to grep's stdin
/// - Failed to read grep's output
/// - Output contains invalid UTF-8
///
/// # Example
/// ```
/// let text = "Issue #123 and #456";
/// let matches = run_grep(text, r"#[0-9]+")?;
/// // matches: "#123\n#456"
/// ```
pub fn run_grep(input: &str, pattern: &str) -> Result<String, String> {
    // Spawn grep process with piped I/O
    let mut child = Command::new("grep")
        .args(["-oE", pattern])
        .stdin(Stdio::piped()) // Write input via pipe
        .stdout(Stdio::piped()) // Capture output
        .stderr(Stdio::piped()) // Capture errors
        .spawn()
        .map_err(|e| format!("Failed to spawn grep: {e}"))?;

    // Write input text to grep's stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| format!("Failed to write to grep stdin: {e}"))?;
    }

    // Wait for grep to complete and capture output
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for grep: {e}"))?;

    // grep exits with:
    // - 0: matches found
    // - 1: no matches (not an error for this tool)
    // - 2: error (e.g. invalid regex)
    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map_err(|e| format!("Invalid UTF-8 from grep: {e}"));
    }

    if output.status.code() == Some(1) {
        return Ok(String::new());
    }

    let stderr = String::from_utf8(output.stderr)
        .unwrap_or_else(|_| "<non-utf8 stderr>".to_string());
    Err(format!("grep failed: {stderr}"))
}

/// Runs jq to extract or transform JSON data.
///
/// # Arguments
/// * `input` - JSON string to process
/// * `query` - jq query expression
///
/// # Returns
/// The result of the jq query as a string.
///
/// # Options
/// - `-r`: Raw output (no JSON encoding of strings)
/// - stderr redirected to null (errors suppressed)
///
/// # Errors
/// Returns an error if:
/// - `jq` command cannot be spawned
/// - Failed to write input to jq's stdin
/// - Failed to read jq's output
/// - Output contains invalid UTF-8
///
/// # Example
/// ```
/// let json = r#"{"name": "Alice", "age": 30}"#;
/// let name = run_jq(json, ".name")?;
/// // name: "Alice"
/// ```
pub fn run_jq(input: &str, query: &str) -> Result<String, String> {
    // Spawn jq process with piped I/O
    let mut child = Command::new("jq")
        .args(["-r", query])
        .stdin(Stdio::piped()) // Write JSON via pipe
        .stdout(Stdio::piped()) // Capture result
        .stderr(Stdio::piped()) // Capture errors
        .spawn()
        .map_err(|e| format!("Failed to spawn jq: {e}"))?;

    // Write JSON input to jq's stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| format!("Failed to write to jq stdin: {e}"))?;
    }

    // Wait for jq to complete and capture output
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for jq: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)
            .unwrap_or_else(|_| "<non-utf8 stderr>".to_string());
        return Err(format!("jq failed: {stderr}"));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 from jq: {e}"))
}

/// Makes a GraphQL request to the Linear API using curl.
///
/// # Arguments
/// * `query` - GraphQL query or mutation as JSON string
/// * `api_key` - Linear API authentication key
///
/// # Returns
/// The JSON response from the Linear API.
///
/// # Request Details
/// - Method: POST
/// - Endpoint: `https://api.linear.app/graphql`
/// - Headers:
///   - `Content-Type: application/json`
///   - `Authorization: <api_key>`
/// - Body: The query parameter
///
/// # Errors
/// Returns an error if:
/// - `curl` command cannot be spawned
/// - HTTP request fails (non-zero exit code)
/// - Response contains invalid UTF-8
///
/// # Example
/// ```
/// let query = r#"{"query": "{ viewer { name } }"}"#;
/// let response = graphql_request(query, "lin_api_...")?;
/// // response: JSON string with viewer data
/// ```
pub fn graphql_request(query: &str, api_key: &str) -> Result<String, String> {
    // Execute curl command with GraphQL request
    let output = Command::new("curl")
        .args([
            "-sS", // Silent mode, but show errors
            "-f",  // Fail on HTTP errors
            "-X",
            "POST", // HTTP POST method
            "-H",
            "Content-Type: application/json", // JSON content type
            "-H",
            &format!("Authorization: {api_key}"), // API key for auth
            "--data",
            query,                            // GraphQL query as body
            "https://api.linear.app/graphql", // Linear API endpoint
        ])
        .output()
        .map_err(|e| format!("Failed to execute curl: {e}"))?;

    // Check if curl succeeded
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)
            .unwrap_or_else(|_| "<non-utf8 stderr>".to_string());
        return Err(format!("curl request failed: {stderr}"));
    }

    // Convert response to string
    String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 from curl: {e}"))
}
