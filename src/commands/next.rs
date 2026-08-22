//! Calculate next patch version command.
//!
//! This command queries the GitHub API to find the latest release version and
//! calculates the next patch version by incrementing the patch number.
//!
//! # Examples
//!
//! ```bash
//! # Get next version (e.g., "0.1.3")
//! cargo version-info next
//!
//! # Get next tag (e.g., "v0.1.3")
//! cargo version-info next --format tag
//!
//! # Get JSON output
//! cargo version-info next --format json
//!
//! # Use in GitHub Actions (writes to GITHUB_OUTPUT)
//! cargo version-info next --format github-actions
//! ```

use anyhow::{
    Context,
    Result,
};
use async_fs_io::write_bytes;
use cargo_plugin_utils::common::get_owner_repo;
use clap::Parser;

use crate::github;
use crate::version::{
    format_tag,
    parse_version,
};

/// Arguments for the `next` command.
#[derive(Parser, Debug)]
pub struct NextArgs {
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

    /// Output format for the version information.
    ///
    /// - `version`: Print just the next version number (e.g., "0.1.3")
    /// - `tag`: Print the next tag with 'v' prefix (e.g., "v0.1.3")
    /// - `json`: Print JSON with latest, next, and next_tag fields
    /// - `github-actions`: Write to GITHUB_OUTPUT file in GitHub Actions format
    #[arg(long, default_value = "version")]
    format: String,

    /// Path to GitHub Actions output file.
    ///
    /// Only used when `--format github-actions` is specified.
    /// Defaults to the `GITHUB_OUTPUT` environment variable or stdout.
    #[arg(long, env = "GITHUB_OUTPUT")]
    github_output: Option<String>,
}

/// Calculate the next patch version from the latest GitHub release.
///
/// Queries the GitHub Releases API to find the latest release version,
/// then increments the patch number. If no releases exist, returns "0.0.1".
///
/// # Errors
///
/// Returns an error if:
/// - The GitHub repository cannot be detected or accessed
/// - The API request fails (network error, authentication failure, etc.)
/// - The latest release version cannot be parsed
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     NextArgs,
///     next,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = NextArgs::parse_from(&[
///     "cargo",
///     "version-info",
///     "next",
///     "--owner",
///     "owner",
///     "--repo",
///     "repo",
/// ]);
/// next(args).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Example Output
///
/// With `--format version`:
/// ```text
/// 0.1.3
/// ```
///
/// With `--format tag`:
/// ```text
/// v0.1.3
/// ```
///
/// With `--format json`:
/// ```json
/// {"latest":"0.1.2","next":"0.1.3","next_tag":"v0.1.3"}
/// ```
///
/// With `--format github-actions` (writes to GITHUB_OUTPUT):
/// ```text
/// latest_version=0.1.2
/// next_version=0.1.3
/// next_tag=v0.1.3
/// ```
pub async fn next(args: NextArgs) -> Result<()> {
    let (owner, repo) = get_owner_repo(args.owner, args.repo)?;
    let github_token = args.github_token.as_deref();

    let (latest, next) = github::calculate_next_version(&owner, &repo, github_token).await?;

    let next_tag = {
        let (major, minor, patch) = parse_version(&next)?;
        format_tag(major, minor, patch)
    };

    match args.format.as_str() {
        "version" => println!("{}", next),
        "tag" => println!("{}", next_tag),
        "json" => {
            println!(
                "{{\"latest\":\"{}\",\"next\":\"{}\",\"next_tag\":\"{}\"}}",
                latest, next, next_tag
            );
        }
        "github-actions" => {
            let output_file = args.github_output.as_deref().unwrap_or("/dev/stdout");
            let output = format!(
                "latest_version={}\nnext_version={}\nnext_tag={}\n",
                latest, next, next_tag
            );
            write_bytes(output_file, output.as_bytes())
                .await
                .with_context(|| format!("Failed to write to {}", output_file))?;
        }
        _ => anyhow::bail!("Invalid format: {}", args.format),
    }

    Ok(())
}
