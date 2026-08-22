//! Get current version from Cargo.toml command.
//!
//! This command extracts the version from a Cargo.toml file, checking both
//! `[workspace.package]` and `[package]` sections.
//!
//! # Examples
//!
//! ```bash
//! # Get version from current directory's Cargo.toml
//! cargo version-info current
//!
//! # Get version from a specific Cargo.toml (standard cargo flag)
//! cargo version-info current --manifest-path ./path/to/Cargo.toml
//!
//! # Get JSON output
//! cargo version-info current --format json
//!
//! # Use in GitHub Actions
//! cargo version-info current --format github-actions
//! ```

use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use async_fs_io::write_bytes;
use cargo_plugin_utils::common::find_package;
use clap::Parser;

/// Arguments for the `current` command.
#[derive(Parser, Debug)]
pub struct CurrentArgs {
    /// Path to the Cargo.toml manifest file (standard cargo flag).
    ///
    /// When running as a cargo subcommand, this is automatically handled.
    #[arg(long)]
    manifest_path: Option<PathBuf>,

    /// Output format for the version.
    ///
    /// - `version`: Print just the version number (e.g., "0.1.2")
    /// - `json`: Print JSON with version field
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

/// Get the current version from a Cargo.toml manifest file.
///
/// Extracts the version from the manifest, checking `[workspace.package]`
/// first (for workspace members), then falling back to `[package]`.
///
/// # Errors
///
/// Returns an error if:
/// - The manifest file cannot be read
/// - No version field is found in either `[workspace.package]` or `[package]`
/// - The output file cannot be written (for github-actions format)
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     CurrentArgs,
///     current,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = CurrentArgs::parse_from(&["cargo", "version-info", "current"]);
/// current(args).await?;
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
/// With `--format json`:
/// ```json
/// {"version":"0.1.2"}
/// ```
///
/// With `--format github-actions` (writes to GITHUB_OUTPUT):
/// ```text
/// version=0.1.2
/// ```
pub async fn current(args: CurrentArgs) -> Result<()> {
    let mut logger = cargo_plugin_utils::logger::Logger::new();

    logger.status("Reading", "package version");
    // Use find_package which automatically handles --manifest-path and workspace
    // logic
    let package = find_package(args.manifest_path.as_deref())?;
    let version = package.version.to_string();
    logger.finish();

    match args.format.as_str() {
        "version" => println!("{}", version),
        "json" => println!("{{\"version\":\"{}\"}}", version),
        "github-actions" => {
            let output_file = args.github_output.as_deref().unwrap_or("/dev/stdout");
            let output = format!("version={}\n", version);
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

        // Create src directory with a minimal lib.rs for cargo metadata to work
        let src_dir = dir.path().join("src");
        async_fs_io::ensure_dir(&src_dir).await.unwrap();
        async_fs_io::write_bytes(src_dir.join("lib.rs"), b"// Test library\n")
            .await
            .unwrap();

        dir
    }

    #[tokio::test]
    async fn test_current_workspace_version() {
        let _dir = async_fs_io::TempDir::create(std::env::temp_dir())
            .await
            .unwrap();
        // Create workspace root Cargo.toml (no [package] section)
        async_fs_io::write_bytes(
            _dir.path().join("Cargo.toml"),
            br#"
[workspace.package]
version = "0.1.2"

[workspace]
members = ["member1"]
"#,
        )
        .await
        .unwrap();

        // Create member package
        let member_dir = _dir.path().join("member1");
        async_fs_io::ensure_dir(member_dir.join("src"))
            .await
            .unwrap();
        async_fs_io::write_bytes(
            member_dir.join("Cargo.toml"),
            br#"
[package]
name = "member1"
version.workspace = true
"#,
        )
        .await
        .unwrap();
        async_fs_io::write_bytes(member_dir.join("src").join("lib.rs"), b"// Test library\n")
            .await
            .unwrap();

        // Test from the member package directory (where we'd normally run the command)
        let manifest_path = member_dir.join("Cargo.toml");
        let args = CurrentArgs {
            manifest_path: Some(manifest_path),
            format: "version".to_string(),
            github_output: None,
        };
        assert!(current(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_current_package_version() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "1.2.3"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = CurrentArgs {
            manifest_path: Some(manifest_path.clone()),
            format: "version".to_string(),
            github_output: None,
        };
        let result = current(args).await;
        if let Err(e) = &result {
            eprintln!("Error in test_current_package_version: {}", e);
            eprintln!("Manifest path: {:?}", manifest_path);
        }
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_current_json_format() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "0.5.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = CurrentArgs {
            manifest_path: Some(manifest_path),
            format: "json".to_string(),
            github_output: None,
        };
        assert!(current(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_current_github_actions_format() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "2.0.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let output_file = async_fs_io::TempFile::create(std::env::temp_dir())
            .await
            .unwrap();
        let args = CurrentArgs {
            manifest_path: Some(manifest_path),
            format: "github-actions".to_string(),
            github_output: Some(output_file.path().to_string_lossy().to_string()),
        };
        assert!(current(args).await.is_ok());

        let content = async_fs_io::read_string_bounded(output_file.path(), 1024 * 1024)
            .await
            .unwrap();
        assert!(content.contains("version=2.0.0"));
    }

    #[tokio::test]
    async fn test_current_invalid_format() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
        version = "1.0.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = CurrentArgs {
            manifest_path: Some(manifest_path),
            format: "invalid".to_string(),
            github_output: None,
        };
        assert!(current(args).await.is_err());
    }

    #[tokio::test]
    async fn test_current_file_not_found() {
        let args = CurrentArgs {
            manifest_path: Some("/nonexistent/Cargo.toml".into()),
            format: "version".to_string(),
            github_output: None,
        };
        assert!(current(args).await.is_err());
    }

    #[tokio::test]
    async fn test_current_no_version() {
        // Cargo defaults to "0.0.0" when no version is specified, so this should
        // succeed
        let _dir = create_temp_cargo_project(
            r#"
[package]
        name = "test"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = CurrentArgs {
            manifest_path: Some(manifest_path),
            format: "version".to_string(),
            github_output: None,
        };
        // Cargo defaults to 0.0.0, so this should succeed
        let result = current(args).await;
        assert!(result.is_ok());
        // Verify it returns the default version
        // (We can't easily capture stdout in this test, but the function should
        // complete)
    }
}
