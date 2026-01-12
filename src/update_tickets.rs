//! Update Linear tickets to completed state.
//!
//! This module implements the `update-tickets` mode which marks Linear tickets
//! as completed using the Linear GraphQL API. It supports dry-run mode for
//! previewing changes without making actual updates.
//!
//! ## Process
//! For each ticket ID:
//! 1. Query current state from Linear API
//! 2. Skip if already completed (Done/Completed)
//! 3. Skip unless current state name is "Passing"
//! 4. Find the team's completed state ID
//! 5. Update ticket to completed state (unless dry-run)
//!
//! ## Dry-Run Mode
//! When `--dry-run` is enabled:
//! - Queries ticket state but skips mutation
//! - Outputs only tickets that would be updated
//! - Suppresses output for already-completed tickets

use crate::config::Config;
use crate::utils;

const NAME: &str = "update-tickets";

macro_rules! log {
    ($fmt:expr) => {
        utils::log(NAME, format_args!($fmt));
    };
}

/// Runs the update-tickets mode to mark Linear tickets as completed.
///
/// # Input
/// Reads Linear ticket IDs from input sources (stdin or files), one per line.
///
/// # Output
/// - **Normal mode**: Prints Linear ticket URLs to stdout for success cases
///   (including already-completed)
/// - **Dry-run mode**: Prints only tickets that would be updated (not already
///   completed)
/// - **Errors**: Prints error messages and failed ticket URLs to stderr
///
/// # Dry-Run Mode
/// If `config.dry_run` is true:
/// - Prints initial message: "Dry-run mode enabled. The following issues are
///   would be marked as Done or Completed:"
/// - Queries each ticket's state but skips the update mutation
/// - Only outputs URLs for tickets that would be updated
///
/// # Errors
/// Returns an error if:
/// - `LINEAR_API_KEY` is not provided
/// - `LINEAR_ORG` is not provided
/// - Input sources cannot be read
/// - Individual ticket updates may fail (logged to stderr, doesn't stop
///   processing)
pub fn run(config: &Config) -> Result<(), String> {
    // Get Linear API key from config or environment
    let linear_api_key = config.get_linear_api_key()?;
    let linear_org = config.get_linear_org()?;

    // Print dry-run header if in preview mode
    if config.dry_run {
        log!(
            "Dry-run mode enabled. The following issues would be marked as Done or Completed:"
        );
    }

    let mut any_output = false;

    log!("reading ticket IDs from input");
    // Process tickets as they arrive from input (streaming), so an upstream
    // stage can keep the pipeline flowing and we can start updating tickets
    // immediately.
    utils::for_each_input_line(&config.input_sources, |input_line| {
        let input_line = input_line.trim();
        if input_line.is_empty() {
            return Ok(());
        }

        let issue_id = match parse_issue_id(input_line) {
            Ok(issue_id) => issue_id,
            Err(e) => {
                log!("Invalid input {input_line}: {e}");
                log!("{input_line}");
                return Ok(());
            }
        };

        let url = issue_url(&linear_org, &issue_id);
        log!("processing {url}");

        match update_single_ticket(
            &issue_id,
            &linear_org,
            &linear_api_key,
            config.dry_run,
            config.update_all_statuses,
        ) {
            Ok(Some(success_url)) => {
                println!("{success_url}");
                any_output = true;
            }
            Ok(None) => {} // No output (ticket already completed in dry-run)
            Err(e) => {
                // Log error to stderr and output failed URL to stderr
                let url = issue_url(&linear_org, &issue_id);
                log!("Failed to update {url}: {e}");
                log!("{url}");
            }
        }
        Ok(())
    })?;

    log!("done");
    if !any_output {
        log!("no changes made");
    }

    Ok(())
}

/// Updates a single Linear ticket to completed state.
///
/// # Arguments
/// * `issue_id` - The Linear issue identifier (e.g., `ABC-123`)
/// * `org` - The Linear organization identifier (for output URLs)
/// * `api_key` - Linear API authentication key
/// * `dry_run` - If true, preview without making changes
///
/// # Returns
/// - `Ok(Some(url))` - Ticket was updated or would be updated
/// - `Ok(None)` - Ticket already completed (in dry-run mode, suppresses output)
/// - `Err(msg)` - Update failed
///
/// # Dry-Run Behavior
/// If `dry_run` is true:
/// - Queries ticket state to check if it's completed
/// - Returns `Ok(Some(url))` if it would be updated (output URL)
/// - Returns `Ok(None)` otherwise (no output)
/// - Skips all mutation logic (team lookup, state update)
///
/// # Normal Mode Behavior
/// 1. Queries current issue state and team ID
/// 3. If already Done/Completed, returns success without updating
/// 4. If current state is not "Passing", skips without updating
/// 5. Finds the team's completed state ID
/// 6. Updates issue to completed state
/// 7. Returns success
///
/// # Errors
/// Returns an error if:
/// - Linear API queries fail
/// - Team ID or completed state not found
/// - Update mutation fails
fn update_single_ticket(
    issue_id: &str,
    org: &str,
    api_key: &str,
    dry_run: bool,
    update_all_statuses: bool,
) -> Result<Option<String>, String> {
    let url = issue_url(org, issue_id);

    // Query current issue state from Linear API
    let issue_details = get_issue_details(issue_id, api_key)?;
    ensure_no_graphql_errors(&issue_details)?;

    let issue_is_null =
        extract_jq_value(&issue_details, ".data.issue == null")?;
    if issue_is_null == "true" {
        return Err("Issue not found".to_string());
    }

    let current_state_name =
        extract_jq_value(&issue_details, ".data.issue.state.name")?;

    // Check if ticket is already in a completed state (matches
    // scripts/linear.sh semantics).
    let is_completed = state_is_done_or_completed(&current_state_name);
    let is_passing = state_is_passing(&current_state_name);
    let should_update = !is_completed && (update_all_statuses || is_passing);

    log!("Current state: {current_state_name}");

    // In dry-run mode, return early after state check
    if dry_run {
        return if should_update {
            Ok(Some(url))
        } else {
            Ok(None)
        };
    }

    // Skip update if already completed
    if is_completed {
        log!("Issue {issue_id} is already in a completed state, skipping.");
        return Ok(Some(url));
    }

    if !should_update {
        log!(
            "Issue {issue_id} is not in \"Passing\" state, skipping (use --update-all-statuses to override)."
        );
        return Ok(None);
    }

    // Get team ID for this issue
    let team_id = extract_jq_value(&issue_details, ".data.issue.team.id")?;

    if team_id == "null" || team_id.is_empty() {
        return Err(format!("Could not find team ID for issue {issue_id}"));
    }

    log!("Found Team ID: {team_id}");

    // Get workflow states for the team and find completed state ID
    let workflow_states = get_workflow_states(&team_id, api_key)?;
    ensure_no_graphql_errors(&workflow_states)?;
    let completed_state_id = find_completed_state(&workflow_states)?;

    log!("Found completed state ID: {completed_state_id}");

    // Execute the mutation to update issue state
    let update_response =
        update_issue_state(issue_id, &completed_state_id, api_key)?;
    ensure_no_graphql_errors(&update_response)?;

    log!("Successfully updated issue {issue_id} to completed");

    Ok(Some(url))
}

/// Queries Linear API for issue details (state and team).
///
/// # Arguments
/// * `issue_id` - The Linear issue ID (e.g., "ABC-123")
/// * `api_key` - Linear API authentication key
///
/// # Returns
/// JSON response containing issue state and team information.
///
/// # GraphQL Query
/// ```graphql
/// query($issueId: String!) {
///   issue(id: $issueId) {
///     team { id }
///     state { name }
///   }
/// }
/// ```
fn get_issue_details(issue_id: &str, api_key: &str) -> Result<String, String> {
    let query = format!(
        r#"{{"query": "query($issueId: String!) {{ issue(id: $issueId) {{ team {{ id }} state {{ name }} }} }}", "variables": {{"issueId": "{issue_id}"}}}}"#
    );

    utils::graphql_request(&query, api_key)
}

/// Queries Linear API for a team's workflow states.
///
/// # Arguments
/// * `team_id` - The Linear team ID
/// * `api_key` - Linear API authentication key
///
/// # Returns
/// JSON response containing all workflow states for the team.
///
/// # GraphQL Query
/// ```graphql
/// query($teamId: String!) {
///   team(id: $teamId) {
///     states {
///       nodes { id name type }
///     }
///   }
/// }
/// ```
fn get_workflow_states(team_id: &str, api_key: &str) -> Result<String, String> {
    let query = format!(
        r#"{{"query": "query($teamId: String!) {{ team(id: $teamId) {{ states {{ nodes {{ id name type }} }} }} }}", "variables": {{"teamId": "{team_id}"}}}}"#
    );

    utils::graphql_request(&query, api_key)
}

/// Finds a completed state ID from workflow states response.
///
/// # Arguments
/// * `workflow_response` - JSON response from `get_workflow_states`
///
/// # Returns
/// The state ID for a "Completed" or "Done" state.
///
/// # Search Strategy
/// Uses case-insensitive regex to find any state with name matching "completed"
/// or "done". Returns the first matching state ID.
///
/// # Errors
/// Returns an error if no completed/done state is found in the workflow.
fn find_completed_state(workflow_response: &str) -> Result<String, String> {
    // jq query to find first state matching "completed" or "done"
    // (case-insensitive)
    let jq_query = r#".data.team.states.nodes[] | select(.name | test("completed|done"; "i")) | .id"#;
    let state_id = utils::run_jq(workflow_response, jq_query)?;
    let state_id = state_id.lines().next().unwrap_or("").trim();

    if state_id.is_empty() || state_id == "null" {
        return Err("Could not find a 'Completed' or 'Done' state".to_string());
    }

    Ok(state_id.to_string())
}

/// Updates a Linear issue to a specific state.
///
/// # Arguments
/// * `issue_id` - The Linear issue ID
/// * `state_id` - The target state ID (typically a completed state)
/// * `api_key` - Linear API authentication key
///
/// # Returns
/// The GraphQL response on success.
///
/// # GraphQL Mutation
/// ```graphql
/// mutation($issueId: String!, $stateId: String!) {
///   issueUpdate(id: $issueId, input: { stateId: $stateId }) {
///     success
///   }
/// }
/// ```
///
/// # Errors
/// Returns an error if the mutation returns `success: false`.
fn update_issue_state(
    issue_id: &str,
    state_id: &str,
    api_key: &str,
) -> Result<String, String> {
    let query = format!(
        r#"{{"query": "mutation($issueId: String!, $stateId: String!) {{ issueUpdate(id: $issueId, input: {{ stateId: $stateId }}) {{ success }} }}", "variables": {{"issueId": "{issue_id}", "stateId": "{state_id}"}}}}"#
    );

    let response = utils::graphql_request(&query, api_key)?;
    let success = extract_jq_value(&response, ".data.issueUpdate.success")?;

    if success == "true" {
        Ok(response)
    } else {
        Err(format!("Update failed: {response}"))
    }
}

/// Extracts a single value from JSON using jq.
///
/// # Arguments
/// * `json` - The JSON string to parse
/// * `query` - The jq query expression
///
/// # Returns
/// The extracted value, trimmed of whitespace.
///
/// # Usage
/// This is a convenience wrapper around `run_jq` that trims the result.
fn extract_jq_value(json: &str, query: &str) -> Result<String, String> {
    let result = utils::run_jq(json, query)?;
    Ok(result.trim().to_string())
}

fn issue_url(org: &str, issue_id: &str) -> String {
    format!("https://linear.app/{org}/issue/{issue_id}")
}

fn parse_issue_id(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty ticket ID".to_string());
    }
    if !is_valid_ticket_id(trimmed) {
        return Err("Expected ticket ID like ABC-123".to_string());
    }
    Ok(trimmed.to_string())
}

fn is_valid_ticket_id(input: &str) -> bool {
    let Some((prefix, digits)) = input.split_once('-') else {
        return false;
    };
    if prefix.len() != 3 || !prefix.bytes().all(|b| b.is_ascii_uppercase()) {
        return false;
    }
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn ensure_no_graphql_errors(response_json: &str) -> Result<(), String> {
    // Linear GraphQL can return HTTP 200 with an `errors` field. Treat that as
    // a failure.
    let messages = utils::run_jq(response_json, ".errors[]?.message // empty")?;
    if messages.trim().is_empty() {
        return Ok(());
    }
    Err(format!("Linear API returned errors: {}", messages.trim()))
}

fn state_is_done_or_completed(state_name: &str) -> bool {
    state_name.contains("Done") || state_name.contains("Completed")
}

fn state_is_passing(state_name: &str) -> bool {
    state_name.eq_ignore_ascii_case("Passing")
}
