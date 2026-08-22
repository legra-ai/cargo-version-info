//! Get latest GitHub release version command.
//!
//! This command queries the GitHub Releases API to find the latest release
//! version for a repository.
//!
//! # Examples
//!
//! ```bash
//! # Get latest version (e.g., "0.1.2")
//! cargo version-info latest
//!
//! # Get latest tag (e.g., "v0.1.2")
//! cargo version-info latest --format tag
//!
//! # Get JSON output
//! cargo version-info latest --format json
//!
//! # Specify repository explicitly
//! cargo version-info latest --owner owner --repo repo
//! ```

use anyhow::Result;
use cargo_plugin_utils::common::get_owner_repo;
use clap::Parser;

use crate::github;
use crate::version::{
    format_tag,
    parse_version,
};

/// Arguments for the `latest` command.
#[derive(Parser, Debug)]
pub struct LatestArgs {
    /// GitHub repository owner.
    ///
    /// Defaults to `GITHUB_REPOSITORY` environment variable (set by GitHub
    /// Actions) or auto-detected from the current git remote.
    #[arg(long)]
    owner: Option<String>,

    /// GitHub repository name.
    ///
    /// Defaults to `GITHUB_REPOSITORY` environment variable (set by GitHub
    /// Actions) or auto-detected from the current git remote.
    #[arg(long)]
    repo: Option<String>,

    /// GitHub personal access token for API authentication.
    ///
    /// Defaults to `GITHUB_TOKEN` environment variable. Required for private
    /// repositories or to avoid rate limiting on public repositories.
    #[arg(long, env = "GITHUB_TOKEN")]
    github_token: Option<String>,

    /// Output format for the version.
    ///
    /// - `version`: Print just the version number (e.g., "0.1.2")
    /// - `tag`: Print the tag with 'v' prefix (e.g., "v0.1.2")
    /// - `json`: Print JSON with version and tag fields
    #[arg(long, default_value = "version")]
    format: String,
}

/// Get the latest GitHub release version for a repository.
///
/// Queries the GitHub Releases API to find the most recent release version.
/// Returns "0.0.0" if no releases exist.
///
/// # Errors
///
/// Returns an error if:
/// - The GitHub repository cannot be detected or accessed
/// - The API request fails (network error, authentication failure, etc.)
/// - The release version cannot be parsed
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     LatestArgs,
///     latest,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = LatestArgs::parse_from(&[
///     "cargo",
///     "version-info",
///     "latest",
///     "--owner",
///     "owner",
///     "--repo",
///     "repo",
/// ]);
/// latest(args).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Example Output
///
/// With `--format version`:
/// ```text
/// 0.1.2
/// ```
///
/// With `--format tag`:
/// ```text
/// v0.1.2
/// ```
///
/// With `--format json`:
/// ```json
/// {"version":"0.1.2","tag":"v0.1.2"}
/// ```
pub async fn latest(args: LatestArgs) -> Result<()> {
    let (owner, repo) = get_owner_repo(args.owner, args.repo)?;
    let github_token = args.github_token.as_deref();

    let latest = github::get_latest_release_version(&owner, &repo, github_token).await?;

    let latest = latest.unwrap_or_else(|| "0.0.0".to_string());

    match args.format.as_str() {
        "version" => println!("{}", latest),
        "tag" => {
            let (major, minor, patch) = parse_version(&latest)?;
            println!("{}", format_tag(major, minor, patch));
        }
        "json" => {
            println!("{{\"version\":\"{}\",\"tag\":\"{}\"}}", latest, {
                let (major, minor, patch) = parse_version(&latest)?;
                format_tag(major, minor, patch)
            });
        }
        _ => anyhow::bail!("Invalid format: {}", args.format),
    }

    Ok(())
}
