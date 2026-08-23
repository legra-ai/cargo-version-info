//! Generate PR log from merged pull requests.
//!
//! This command generates a markdown list of merged pull requests since
//! a given tag, useful for release notes.
//!
//! # Examples
//!
//! ```bash
//! # Generate PR log since last tag
//! cargo version-info pr-log
//!
//! # Generate PR log since specific tag
//! cargo version-info pr-log --since-tag v0.1.0
//!
//! # Output to file
//! cargo version-info pr-log --output PR_LOG.md
//! ```

use anyhow::Result;
use clap::Parser;

/// Arguments for the `pr-log` command.
#[derive(Parser, Debug)]
pub struct PrLogArgs {
    /// Tag to compare from (default: latest tag).
    #[arg(long)]
    pub since_tag: Option<String>,

    /// Output file path (default: stdout).
    #[arg(short, long)]
    pub output: Option<String>,

    /// GitHub repository owner.
    #[arg(long)]
    pub owner: Option<String>,

    /// GitHub repository name.
    #[arg(long)]
    pub repo: Option<String>,
}

/// Generate PR log from merged pull requests.
///
/// # Note
///
/// This command is currently a stub and not yet implemented. It will be
/// available in a future release.
pub async fn pr_log(_args: PrLogArgs) -> Result<()> {
    anyhow::bail!(
        "PR log generation is not yet implemented. This feature will be available in a future release."
    );
}
