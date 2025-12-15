# release-linear-ticket-update

A Rust tool for automatically marking Linear tickets as completed when a GitHub release is published.

## Overview

This tool processes GitHub release notes to find associated Pull Requests, extracts Linear ticket IDs from those PRs, and marks them as completed in Linear. It can be used as individual pipeline commands or as a single orchestrated workflow.

## Prerequisites

The tool relies on these external commands being available:
- `gh` (GitHub CLI)
- `curl` (for Linear API requests)
- `grep` (for pattern matching)
- `jq` (for JSON manipulation)

No other dependencies are assumed.

## Building

```bash
cargo build --release
```

The binary will be available at `target/release/release-linear-ticket-update`

## Modes

### 1. Parse Release Notes (`parse-notes`)

Extracts Pull Request numbers from release notes.

**Usage:**
```bash
# From a release tag
release-linear-ticket-update parse-notes --release-tag v1.2.3

# From stdin
echo "Fixed #123 and #456" | release-linear-ticket-update parse-notes
```

**Output:** List of PR numbers (one per line)

### 2. Extract Linear Tickets (`extract-tickets`)

Finds Linear ticket IDs from Pull Requests by examining PR title, body, comments, and commit messages.

**Usage:**
```bash
# From file
release-linear-ticket-update extract-tickets pr_numbers.txt

# From stdin
echo "123" | release-linear-ticket-update extract-tickets

# Explicit stdin with files
release-linear-ticket-update extract-tickets - other_prs.txt

# Chained from parse-notes
release-linear-ticket-update parse-notes --release-tag v1.2.3 | release-linear-ticket-update extract-tickets
```

**Output:** List of Linear ticket IDs (one per line), e.g. `ABC-123`

### 3. Update Linear Tickets (`update-tickets`)

Marks Linear tickets as completed using the Linear GraphQL API.

**Usage:**
```bash
# From file (with env var)
LINEAR_API_KEY=your_key release-linear-ticket-update update-tickets tickets.txt

# With flag
release-linear-ticket-update update-tickets --linear-api-key your_key --linear-org myorg tickets.txt

# From stdin
echo "ABC-123" | release-linear-ticket-update update-tickets --linear-api-key your_key --linear-org myorg

# Chained pipeline
release-linear-ticket-update extract-tickets prs.txt | release-linear-ticket-update update-tickets --linear-api-key your_key --linear-org myorg

# Dry-run mode (check what would be updated without making changes)
release-linear-ticket-update update-tickets --dry-run --linear-api-key your_key --linear-org myorg tickets.txt

# Update regardless of current Linear state
release-linear-ticket-update update-tickets --update-all-statuses --linear-api-key your_key --linear-org myorg tickets.txt
```

**Required:**
- `--linear-api-key` flag or `LINEAR_API_KEY` environment variable
- `--linear-org` flag or `LINEAR_ORG` environment variable

**Optional:**
- `--dry-run` flag: Preview which tickets would be updated without actually updating them
- `--update-all-statuses` flag: Update tickets even if they are not currently in "Passing" state

**Output:**
- stdout: Successfully updated ticket URLs (or URLs that would be updated in dry-run mode)
- stderr: Failed ticket URLs and error messages

**Dry-run Mode:**
When `--dry-run` is specified:
- Still queries Linear API to check each ticket's current state
- Outputs only tickets that would be updated
- Skips the actual mutation (does not update tickets)
- Useful for previewing changes before running the actual update

**Workflow State Filtering:**
By default, tickets are only updated if their current state name is "Passing" (case-insensitive). Use `--update-all-statuses` to update any ticket that is not already Done/Completed.

### 4. Orchestrator Mode (default)

Runs the complete pipeline: parse-notes → extract-tickets → update-tickets

**Usage:**
```bash
# Full workflow
release-linear-ticket-update --release-tag v1.2.3

# With environment variables
LINEAR_API_KEY=key LINEAR_ORG=myorg release-linear-ticket-update --release-tag v1.2.3

# With flags
release-linear-ticket-update --release-tag v1.2.3 --linear-api-key key --linear-org myorg

# Dry-run mode
release-linear-ticket-update --dry-run --release-tag v1.2.3 --linear-api-key key --linear-org myorg

# Update regardless of current Linear state
release-linear-ticket-update --update-all-statuses --release-tag v1.2.3 --linear-api-key key --linear-org myorg
```

**Required:**
- `--release-tag` flag
- `LINEAR_API_KEY` (via flag or env var)
- `LINEAR_ORG` (via flag or env var)

**Optional:**
- `--dry-run` flag: Preview which tickets would be updated without making changes
- `--update-all-statuses` flag: Update tickets even if they are not currently in "Passing" state

## Examples

### Basic Workflow

```bash
# Set environment variables
export LINEAR_API_KEY="lin_api_xxx"
export LINEAR_ORG="myorg"

# Run complete workflow
release-linear-ticket-update --release-tag v2.0.0
```

### Manual Pipeline

```bash
# Step 1: Extract PR numbers from release
release-linear-ticket-update parse-notes --release-tag v2.0.0 > prs.txt

# Step 2: Find Linear tickets in those PRs
release-linear-ticket-update extract-tickets prs.txt > tickets.txt

# Step 3: Mark tickets as completed
release-linear-ticket-update update-tickets --linear-api-key "$LINEAR_API_KEY" tickets.txt
```

### One-liner Pipeline

```bash
release-linear-ticket-update parse-notes --release-tag v2.0.0 | \
  release-linear-ticket-update extract-tickets | \
  release-linear-ticket-update update-tickets --linear-api-key "$LINEAR_API_KEY" --linear-org "$LINEAR_ORG"
```

## Logging

Progress output is written to stderr and prefixed with a fixed-width stage name for easy scanning:

```text
parse-notes     : ...
extract-tickets : ...
update-tickets  : ...
```

All three pipeline stages are streaming (they do not read all stdin before starting work), so this works as expected:

```bash
release-linear-ticket-update parse-notes --release-tag v2.0.0 | \
  release-linear-ticket-update extract-tickets | \
  release-linear-ticket-update update-tickets --linear-api-key "$LINEAR_API_KEY" --linear-org "$LINEAR_ORG"
```

## GitHub Actions Integration

```yaml
name: Complete Linear Tickets on Release

on:
  release:
    types: [published]

jobs:
  complete-tickets:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Install release-linear-ticket-update
        run: |
          cargo install --locked --git https://github.com/mkpro118/release-linear-ticket-update --bin release-linear-ticket-update
          echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"

      - name: Complete Linear tickets
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          LINEAR_API_KEY: ${{ secrets.LINEAR_API_KEY }}
          LINEAR_ORG: hipphealth
        run: |
          release-linear-ticket-update --release-tag ${{ github.event.release.tag_name }}
```

## Implementation Details

- No external dependencies (uses stdlib only)
- Relies on delegation to system commands (gh, curl, grep, jq) rather than bundling libraries
