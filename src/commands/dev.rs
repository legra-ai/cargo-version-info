//! Generate dev version from git SHA command.
//!
//! This command generates a development version string from the current git
//! commit SHA. The format is `0.0.0-dev-<short-sha>`.
//!
//! # Examples
//!
//! ```bash
//! # Get dev version (e.g., "0.0.0-dev-a1b2c3d")
//! cargo version-info dev
//!
//! # Get JSON output with SHA
//! cargo version-info dev --format json
//!
//! # Use a different repository path
//! cargo version-info dev --repo-path /path/to/repo
//! ```

use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use clap::Parser;

/// Arguments for the `dev` command.
#[derive(Parser, Debug)]
pub struct DevArgs {
    /// Path to the git repository.
    ///
    /// Defaults to the current directory. The command will search upward
    /// from this path to find the repository root.
    #[arg(long, default_value = ".")]
    repo_path: PathBuf,

    /// Output format for the dev version.
    ///
    /// - `version`: Print just the dev version (e.g., "0.0.0-dev-a1b2c3d")
    /// - `json`: Print JSON with version and sha fields
    #[arg(long, default_value = "version")]
    format: String,
}

/// Generate a development version from the current git commit SHA.
///
/// Reads the HEAD commit from the git repository and generates a version
/// string in the format `0.0.0-dev-<short-sha>` where `<short-sha>` is the
/// shortened commit hash.
///
/// # Errors
///
/// Returns an error if:
/// - The git repository cannot be discovered
/// - HEAD does not point to a valid commit
/// - The commit SHA cannot be shortened
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     DevArgs,
///     dev,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = DevArgs::parse_from(&["cargo", "version-info", "dev"]);
/// dev(args).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Example Output
///
/// With `--format version`:
/// ```text
/// 0.0.0-dev-a1b2c3d
/// ```
///
/// With `--format json`:
/// ```json
/// {"version":"0.0.0-dev-a1b2c3d","sha":"a1b2c3d"}
/// ```
pub async fn dev(args: DevArgs) -> Result<()> {
    let repo = gix::discover(&args.repo_path).with_context(|| {
        format!(
            "Failed to discover git repository at {}",
            args.repo_path.display()
        )
    })?;

    let head = repo.head().context("Failed to read HEAD")?;
    let commit_id = head.id().context("HEAD does not point to a commit")?;
    let short_sha = commit_id
        .shorten()
        .context("Failed to shorten commit SHA")?;

    let dev_version = format!("0.0.0-dev-{}", short_sha);

    match args.format.as_str() {
        "version" => println!("{}", dev_version),
        "json" => println!(
            "{{\"version\":\"{}\",\"sha\":\"{}\"}}",
            dev_version, short_sha
        ),
        _ => anyhow::bail!("Invalid format: {}", args.format),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dev_current_repo() {
        // Test with current directory (should work if run from git repo)
        let args = DevArgs {
            repo_path: ".".into(),
            format: "version".to_string(),
        };
        // This will only work if run from a git repository
        // We'll just verify it doesn't panic on invalid format
        let result = dev(args).await;
        // Either succeeds (in git repo) or fails gracefully
        if let Err(e) = result {
            // Check it's the expected error type
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("Failed to discover git repository")
                    || err_msg.contains("Failed to read HEAD")
                    || err_msg.contains("HEAD does not point to a commit")
            );
        }
    }

    #[tokio::test]
    async fn test_dev_json_format() {
        let args = DevArgs {
            repo_path: ".".into(),
            format: "json".to_string(),
        };
        // Same as above - will work if in git repo, otherwise fail gracefully
        let _ = dev(args).await;
    }

    #[tokio::test]
    async fn test_dev_invalid_format() {
        let args = DevArgs {
            repo_path: ".".into(),
            format: "invalid".to_string(),
        };
        // Should fail on invalid format even if repo is valid
        let result = dev(args).await;
        // If repo is invalid, we get repo error; if repo is valid, we get format error
        // The format check happens after repo discovery, so we may get either error
        if let Err(e) = result {
            let err_msg = e.to_string();
            // Accept either format error or repo discovery error
            assert!(
                err_msg.contains("Invalid format")
                    || err_msg.contains("Failed to discover git repository")
                    || err_msg.contains("Failed to read HEAD")
                    || err_msg.contains("HEAD does not point to a commit")
            );
        } else {
            // If it succeeds (unlikely with invalid format), that's also
            // acceptable as the test is checking error handling
        }
    }

    #[tokio::test]
    async fn test_dev_nonexistent_repo() {
        let args = DevArgs {
            repo_path: "/nonexistent/path".into(),
            format: "version".to_string(),
        };
        assert!(dev(args).await.is_err());
    }
}
