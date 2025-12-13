//! Configuration and command-line argument parsing for the application.
//!
//! This module handles parsing CLI arguments and building the configuration
//! used by all operational modes. It supports flexible Unix-style input
//! handling with stdin represented by "-".

use std::env;

/// Operational mode for the application.
///
/// The application can run in four distinct modes:
/// - Individual pipeline stages (parse-notes, extract-tickets, update-tickets)
/// - Orchestrator mode that chains all stages together
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    /// Parse release notes to extract PR numbers
    ParseNotes,
    /// Extract Linear ticket IDs from PR content
    ExtractTickets,
    /// Update Linear tickets to completed state
    UpdateTickets,
    /// Run the complete pipeline (parse -> extract -> update)
    Orchestrator,
}

/// Source of input data for commands.
///
/// Supports Unix-style input handling where stdin can be explicitly
/// requested with "-" or implicitly used when no files are provided.
#[derive(Debug)]
pub enum InputSource {
    /// Read from standard input
    Stdin,
    /// Read from a file at the given path
    File(String),
}

/// Application configuration parsed from command-line arguments.
///
/// This struct holds all configuration needed to run the application,
/// including the operational mode, API credentials, and input sources.
#[derive(Debug)]
pub struct Config {
    /// The operational mode to run
    pub mode: Mode,
    /// GitHub release tag (required for parse-notes and orchestrator modes)
    pub release_tag: Option<String>,
    /// Linear API key for authentication (can also come from environment)
    pub linear_api_key: Option<String>,
    /// Linear organization identifier (can also come from environment)
    pub linear_org: Option<String>,
    /// Input sources (files or stdin) for processing
    pub input_sources: Vec<InputSource>,
    /// Whether to run in dry-run mode (preview without making changes)
    pub dry_run: bool,
}

impl Config {
    /// Gets the Linear API key from config or environment variable.
    ///
    /// # Precedence
    /// 1. --linear-api-key CLI flag
    /// 2. `LINEAR_API_KEY` environment variable
    ///
    /// # Errors
    /// Returns an error if neither the flag nor environment variable is set.
    pub fn get_linear_api_key(&self) -> Result<String, String> {
        let env_api_key = env::var("LINEAR_API_KEY").ok();
        self.linear_api_key
            .as_ref()
            .or(env_api_key.as_ref())
            .ok_or_else(|| {
                "LINEAR_API_KEY not provided via --linear-api-key flag or environment variable"
                    .to_string()
            })
            .map(String::from)
    }

    /// Gets the Linear organization identifier from config or environment
    /// variable.
    ///
    /// # Precedence
    /// 1. --linear-org CLI flag
    /// 2. `LINEAR_ORG` environment variable
    ///
    /// # Errors
    /// Returns an error if neither the flag nor environment variable is set.
    pub fn get_linear_org(&self) -> Result<String, String> {
        let env_linear_org = env::var("LINEAR_ORG").ok();
        self.linear_org
            .as_ref()
            .or(env_linear_org.as_ref())
            .ok_or_else(|| {
                "LINEAR_ORG not provided via --linear-org flag or environment variable".to_string()
            })
            .map(String::from)
    }

    /// Parses command-line arguments into a Config struct.
    ///
    /// # Argument Format
    /// ```text
    /// release-linear-ticket-update [MODE] [OPTIONS] [FILES...]
    ///
    /// Modes:
    ///   parse-notes        Parse release notes for PR numbers
    ///   extract-tickets    Extract Linear tickets from PRs
    ///   update-tickets     Update Linear tickets to completed
    ///   (default)          Run orchestrator mode (full pipeline)
    ///
    /// Options:
    ///   --release-tag TAG      GitHub release tag
    ///   --linear-api-key KEY   Linear API authentication key
    ///   --linear-org ORG       Linear organization identifier
    ///   --dry-run              Preview changes without updating
    ///
    /// Files:
    ///   -                      Read from stdin (can be mixed with files)
    ///   file1 file2 ...        Read from these files
    /// ```
    ///
    /// # Defaults
    /// - If no mode specified, defaults to Orchestrator
    /// - If no input sources for extract-tickets/update-tickets, defaults to
    ///   stdin
    ///
    /// # Errors
    /// Returns an error if stdin (-) is specified more than once.
    pub fn from_args() -> Result<Self, String> {
        let args: Vec<String> = env::args().collect();

        let (mode, start_idx) = parse_mode_and_start_index(&args)?;
        let mut parsed = parse_flags_and_inputs(mode, &args, start_idx)?;
        apply_defaults(mode, &mut parsed);
        validate_config(mode, &parsed)?;

        Ok(Self {
            mode,
            release_tag: parsed.release_tag,
            linear_api_key: parsed.linear_api_key,
            linear_org: parsed.linear_org,
            input_sources: parsed.input_sources,
            dry_run: parsed.dry_run,
        })
    }
}

#[derive(Debug)]
struct ParsedArgs {
    release_tag: Option<String>,
    linear_api_key: Option<String>,
    linear_org: Option<String>,
    input_sources: Vec<InputSource>,
    dry_run: bool,
}

fn parse_mode_and_start_index(
    args: &[String],
) -> Result<(Mode, usize), String> {
    let first = args.get(1).ok_or_else(|| {
        "Orchestrator mode requires --release-tag flag".to_string()
    })?;
    if first.starts_with("--") {
        return Ok((Mode::Orchestrator, 1));
    }
    let mode = match first.as_str() {
        "parse-notes" => Mode::ParseNotes,
        "extract-tickets" => Mode::ExtractTickets,
        "update-tickets" => Mode::UpdateTickets,
        other => return Err(format!("Unknown mode: {other}")),
    };
    Ok((mode, 2))
}

fn parse_flags_and_inputs(
    mode: Mode,
    args: &[String],
    start_idx: usize,
) -> Result<ParsedArgs, String> {
    let mut parsed = ParsedArgs {
        release_tag: None,
        linear_api_key: None,
        linear_org: None,
        input_sources: Vec::new(),
        dry_run: false,
    };

    let mut stdin_used = false;
    let mut i = start_idx;
    while i < args.len() {
        let arg = args
            .get(i)
            .ok_or_else(|| "Internal error while parsing args".to_string())?;

        if arg == "--dry-run" {
            parsed.dry_run = true;
            i += 1;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--release-tag=") {
            parsed.release_tag = Some(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--release-tag" {
            let value = args
                .get(i + 1)
                .ok_or_else(|| "Missing value for --release-tag".to_string())?;
            parsed.release_tag = Some(value.clone());
            i += 2;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--linear-api-key=") {
            parsed.linear_api_key = Some(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--linear-api-key" {
            let value = args.get(i + 1).ok_or_else(|| {
                "Missing value for --linear-api-key".to_string()
            })?;
            parsed.linear_api_key = Some(value.clone());
            i += 2;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--linear-org=") {
            parsed.linear_org = Some(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--linear-org" {
            let value = args
                .get(i + 1)
                .ok_or_else(|| "Missing value for --linear-org".to_string())?;
            parsed.linear_org = Some(value.clone());
            i += 2;
            continue;
        }

        if arg == "-" {
            match mode {
                Mode::ExtractTickets | Mode::UpdateTickets => {
                    if stdin_used {
                        return Err(
                            "stdin (-) cannot be specified more than once"
                                .to_string(),
                        );
                    }
                    parsed.input_sources.push(InputSource::Stdin);
                    stdin_used = true;
                }
                Mode::ParseNotes => {
                    return Err(
                        "parse-notes reads from stdin implicitly; '-' is not accepted".to_string(),
                    );
                }
                Mode::Orchestrator => {
                    return Err(
                        "Orchestrator mode does not accept stdin ('-')"
                            .to_string(),
                    );
                }
            }
            i += 1;
            continue;
        }

        if arg.starts_with("--") {
            return Err(format!("Unknown flag: {arg}"));
        }

        match mode {
            Mode::ExtractTickets | Mode::UpdateTickets => {
                parsed.input_sources.push(InputSource::File(arg.clone()));
            }
            Mode::ParseNotes => {
                return Err(
                    "parse-notes does not accept file arguments".to_string()
                );
            }
            Mode::Orchestrator => {
                return Err("Orchestrator mode does not accept file arguments"
                    .to_string());
            }
        }
        i += 1;
    }

    Ok(parsed)
}

fn apply_defaults(mode: Mode, parsed: &mut ParsedArgs) {
    if parsed.input_sources.is_empty()
        && matches!(mode, Mode::ExtractTickets | Mode::UpdateTickets)
    {
        parsed.input_sources.push(InputSource::Stdin);
    }
}

fn validate_config(mode: Mode, parsed: &ParsedArgs) -> Result<(), String> {
    match mode {
        Mode::ParseNotes => {
            if parsed.linear_api_key.is_some()
                || parsed.linear_org.is_some()
                || parsed.dry_run
            {
                return Err(
                    "parse-notes does not accept Linear credentials or --dry-run".to_string(),
                );
            }
        }
        Mode::ExtractTickets => {
            if parsed.release_tag.is_some()
                || parsed.linear_api_key.is_some()
                || parsed.linear_org.is_some()
                || parsed.dry_run
            {
                return Err(
                    "extract-tickets does not accept --release-tag, Linear credentials, or --dry-run".to_string(),
                );
            }
        }
        Mode::UpdateTickets => {
            if parsed.release_tag.is_some() {
                return Err(
                    "update-tickets does not accept --release-tag".to_string()
                );
            }
        }
        Mode::Orchestrator => {
            if parsed.release_tag.is_none() {
                return Err(
                    "Orchestrator mode requires --release-tag flag".to_string()
                );
            }
            if !parsed.input_sources.is_empty() {
                return Err(
                    "Orchestrator mode does not accept input files".to_string()
                );
            }
        }
    }
    Ok(())
}
