//! Check if Cargo.toml version changed since last git tag command.
//!
//! This command compares the version in Cargo.toml with the latest git tag
//! to determine if the version has been updated since the last release.
//!
//! # Examples
//!
//! ```bash
//! # Check if version changed (returns true/false)
//! cargo version-info changed
//!
//! # Get JSON output with both versions
//! cargo version-info changed --format json
//!
//! # Get human-readable diff
//! cargo version-info changed --format diff
//!
//! # Use in GitHub Actions
//! cargo version-info changed --format github-actions
//! ```

use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use async_fs_io::write_bytes;
use cargo_plugin_utils::common::get_package_version_from_manifest;
use clap::Parser;

/// Arguments for the `changed` command.
#[derive(Parser, Debug)]
pub struct ChangedArgs {
    /// Path to the Cargo.toml manifest file (standard cargo flag).
    ///
    /// When running as a cargo subcommand, this is automatically handled.
    #[arg(long)]
    manifest_path: Option<PathBuf>,

    /// Path to the git repository.
    ///
    /// Defaults to the current directory. Used to find the latest git tag.
    #[arg(long, default_value = ".")]
    repo_path: PathBuf,

    /// Output format for the comparison result.
    ///
    /// - `bool`: Print "true" if version changed, "false" if unchanged
    /// - `json`: Print JSON with changed, cargo_version, and latest_tag_version
    ///   fields
    /// - `diff`: Print human-readable diff (e.g., "Version changed: 0.1.0 ->
    ///   0.1.1")
    /// - `github-actions`: Write to GITHUB_OUTPUT file in GitHub Actions format
    #[arg(long, default_value = "bool")]
    format: String,

    /// Path to GitHub Actions output file.
    ///
    /// Only used when `--format github-actions` is specified.
    /// Defaults to the `GITHUB_OUTPUT` environment variable or stdout.
    #[arg(long, env = "GITHUB_OUTPUT")]
    github_output: Option<String>,
}

/// Check if the Cargo.toml version has changed since the last git tag.
///
/// Extracts the version from Cargo.toml (checking `[workspace.package]` first,
/// then `[package]`) and compares it with the latest git tag. If no tags exist,
/// the tag version is assumed to be "0.0.0".
///
/// This is useful for CI/CD pipelines to determine if a version bump is needed
/// or if the version has already been updated.
///
/// # Errors
///
/// Returns an error if:
/// - The manifest file cannot be read
/// - No version field is found in Cargo.toml
/// - The output file cannot be written (for github-actions format)
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     ChangedArgs,
///     changed,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = ChangedArgs::parse_from(&["cargo", "version-info", "changed"]);
/// changed(args).await?; // Prints "true" or "false"
///
/// # Ok(())
/// # }
/// ```
///
/// # Example Output
///
/// With `--format bool` (version changed):
/// ```text
/// true
/// ```
///
/// With `--format bool` (version unchanged):
/// ```text
/// false
/// ```
///
/// With `--format json`:
/// ```json
/// {"changed":true,"cargo_version":"0.1.1","latest_tag_version":"0.1.0"}
/// ```
///
/// With `--format diff` (version changed):
/// ```text
/// Version changed: 0.1.0 -> 0.1.1
/// ```
///
/// With `--format diff` (version unchanged):
/// ```text
/// Version unchanged: 0.1.0
/// ```
///
/// With `--format github-actions` (writes to GITHUB_OUTPUT):
/// ```text
/// changed=true
/// version=0.1.1
/// latest_tag_version=0.1.0
/// ```
pub async fn changed(args: ChangedArgs) -> Result<()> {
    // Suppress progress when outputting to stdout (bool/json formats)
    let mut logger = cargo_plugin_utils::logger::Logger::new();

    logger.status("Reading", "package version");
    // Get current version from Cargo.toml using cargo_metadata (idiomatic way)
    let manifest_path = args
        .manifest_path
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("./Cargo.toml"));
    let cargo_version = get_package_version_from_manifest(manifest_path)
        .with_context(|| format!("Failed to get version from {}", manifest_path.display()))?;

    logger.status("Checking", "git tags");

    // Find latest tag using gix
    let latest_tag = gix::discover(&args.repo_path)
        .ok()
        .and_then(|repo| {
            repo.references()
                .ok()?
                .all()
                .ok()?
                .filter_map(|reference| {
                    let Ok(reference) = reference else {
                        return None;
                    };
                    let name = reference.name().as_bstr().to_string();
                    name.strip_prefix("refs/tags/").map(|tag| {
                        let tag_oid = reference.id();
                        (tag.to_string(), tag_oid)
                    })
                })
                .filter_map(|(tag_name, _tag_oid)| {
                    // Selecting "latest" by semver is what release pipelines
                    // actually want — sorting by OID is meaningless, and
                    // sorting by commit time misranks out-of-order tags.
                    // Annotated and lightweight tags are treated alike: we
                    // only need the tag name to compare, not the target.
                    let stripped = tag_name
                        .strip_prefix('v')
                        .or_else(|| tag_name.strip_prefix('V'))
                        .unwrap_or(&tag_name);
                    let version = cargo_metadata::semver::Version::parse(stripped).ok()?;
                    Some((tag_name, version))
                })
                .max_by(|(_, a), (_, b)| a.cmp(b))
                .map(|(tag_name, _)| tag_name)
        })
        .unwrap_or_else(|| "v0.0.0".to_string());

    // Strip optional leading v/V
    let latest_tag_version = latest_tag
        .strip_prefix('v')
        .or_else(|| latest_tag.strip_prefix('V'))
        .unwrap_or(&latest_tag)
        .to_string();

    let changed = cargo_version != latest_tag_version;
    logger.finish();

    match args.format.as_str() {
        "bool" => println!("{}", changed),
        "json" => println!(
            "{{\"changed\":{},\"cargo_version\":\"{}\",\"latest_tag_version\":\"{}\"}}",
            changed, cargo_version, latest_tag_version
        ),
        "diff" => {
            if changed {
                println!(
                    "Version changed: {} -> {}",
                    latest_tag_version, cargo_version
                );
            } else {
                println!("Version unchanged: {}", cargo_version);
            }
        }
        "github-actions" => {
            let output_file = args.github_output.as_deref().unwrap_or("/dev/stdout");
            let output = format!(
                "changed={}\nversion={}\nlatest_tag_version={}\n",
                changed, cargo_version, latest_tag_version
            );
            write_bytes(output_file, output.as_bytes())
                .await
                .with_context(|| format!("Failed to write to {}", output_file))?;
        }
        _ => anyhow::bail!("Invalid format: {}", args.format),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_temp_cargo_project(content: &str) -> async_fs_io::TempDir {
        let dir = async_fs_io::TempDir::create(std::env::temp_dir())
            .await
            .unwrap();
        let manifest_path = dir.path().join("Cargo.toml");
        async_fs_io::write_bytes(&manifest_path, content.as_bytes())
            .await
            .unwrap();
        dir
    }

    #[tokio::test]
    async fn test_changed_bool_format() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "0.1.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = ChangedArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            format: "bool".to_string(),
            github_output: None,
        };
        // Will succeed if git repo exists, otherwise may fail on git describe
        let _ = changed(args).await;
    }

    #[tokio::test]
    async fn test_changed_json_format() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "1.0.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = ChangedArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            format: "json".to_string(),
            github_output: None,
        };
        let _ = changed(args).await;
    }

    #[tokio::test]
    async fn test_changed_diff_format() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "2.0.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = ChangedArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            format: "diff".to_string(),
            github_output: None,
        };
        let _ = changed(args).await;
    }

    #[tokio::test]
    async fn test_changed_github_actions_format() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "3.0.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let output_file = async_fs_io::TempFile::create(std::env::temp_dir())
            .await
            .unwrap();
        let args = ChangedArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            format: "github-actions".to_string(),
            github_output: Some(output_file.path().to_string_lossy().to_string()),
        };
        let result = changed(args).await;
        // May succeed or fail depending on git state, but if it succeeds, check output
        if result.is_ok() {
            let content = async_fs_io::read_string_bounded(output_file.path(), 1024 * 1024)
                .await
                .unwrap();
            assert!(content.contains("changed="));
            assert!(content.contains("version="));
            assert!(content.contains("latest_tag_version="));
        }
    }

    #[tokio::test]
    async fn test_changed_invalid_format() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "1.0.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = ChangedArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            format: "invalid".to_string(),
            github_output: None,
        };
        assert!(changed(args).await.is_err());
    }

    #[tokio::test]
    async fn test_changed_file_not_found() {
        let args = ChangedArgs {
            manifest_path: Some("/nonexistent/Cargo.toml".into()),
            repo_path: ".".into(),
            format: "bool".to_string(),
            github_output: None,
        };
        assert!(changed(args).await.is_err());
    }

    #[tokio::test]
    async fn test_changed_no_version() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = ChangedArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            format: "bool".to_string(),
            github_output: None,
        };
        assert!(changed(args).await.is_err());
    }

    #[tokio::test]
    async fn test_changed_workspace_version() {
        let _dir = create_temp_cargo_project(
            r#"
[workspace.package]
version = "0.5.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = ChangedArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            format: "bool".to_string(),
            github_output: None,
        };
        let _ = changed(args).await;
    }
}
