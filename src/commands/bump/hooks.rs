//! Hook configuration and execution for version bumping.
//!
//! This module provides support for running custom hooks during the version
//! bump process. Hooks allow projects to perform additional actions like
//! syncing version numbers to other files (npm package.json, etc.).
//!
//! # Configuration
//!
//! Hooks are configured in `Cargo.toml` under
//! `[package.metadata.version-info]`:
//!
//! ```toml
//! [package.metadata.version-info]
//! pre_bump_hooks = [
//!     "./scripts/sync-npm-version.sh {{version}}"
//! ]
//! additional_files = [
//!     "npm/package.json"
//! ]
//! post_bump_hooks = [
//!     "echo 'Version {{version}} committed'"
//! ]
//! ```
//!
//! # Hook Types
//!
//! - **pre_bump_hooks**: Run after Cargo.toml is updated but before commit
//! - **post_bump_hooks**: Run after the commit is created
//! - **additional_files**: Files to include in the version bump commit
//!
//! # Template Variables
//!
//! Hooks support the `{{version}}` placeholder which is replaced with the
//! new version string before execution.

use std::path::Path;

use anyhow::{
    Context,
    Result,
};
use serde::Deserialize;

/// Configuration for version-info hooks.
///
/// This struct is deserialized from `[package.metadata.version-info]` in
/// Cargo.toml. All fields are optional and default to empty vectors.
///
/// # Example
///
/// ```toml
/// [package.metadata.version-info]
/// pre_bump_hooks = ["./scripts/sync-version.sh {{version}}"]
/// additional_files = ["package.json", "npm/package.json"]
/// post_bump_hooks = ["echo 'Done!'"]
/// ```
#[derive(Debug, Default, Deserialize)]
pub struct VersionInfoConfig {
    /// Commands to run after updating Cargo.toml but before committing.
    ///
    /// Use these hooks to update other files that need version changes,
    /// such as npm package.json files.
    #[serde(default)]
    pub pre_bump_hooks: Vec<String>,

    /// Commands to run after creating the commit.
    ///
    /// Use these hooks for notifications or follow-up actions.
    #[serde(default)]
    pub post_bump_hooks: Vec<String>,

    /// Additional files to include in the version bump commit.
    ///
    /// These files are staged and committed along with Cargo.toml.
    /// Useful for files modified by pre_bump_hooks.
    #[serde(default)]
    pub additional_files: Vec<String>,
}

impl VersionInfoConfig {
    /// Load configuration from package metadata.
    ///
    /// Reads the `version-info` key from `[package.metadata]` in Cargo.toml.
    /// Returns default (empty) config if the key doesn't exist or parsing
    /// fails.
    ///
    /// # Arguments
    ///
    /// * `package` - The cargo_metadata package to read configuration from
    ///
    /// # Returns
    ///
    /// Returns the parsed configuration or defaults if not found.
    pub fn from_package(package: &cargo_metadata::Package) -> Self {
        package
            .metadata
            .get("version-info")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }
}

/// Run a hook command with version substitution.
///
/// Replaces `{{version}}` placeholders in the command with the actual version
/// and executes it as a shell command.
///
/// # Arguments
///
/// * `command` - The hook command to run (may contain `{{version}}`)
/// * `version` - The version string to substitute
/// * `working_dir` - Directory to run the command in
///
/// # Returns
///
/// Returns `Ok(())` if the command succeeds, or an error if it fails.
///
/// # Errors
///
/// Returns an error if:
/// - The shell cannot be spawned
/// - The command exits with a non-zero status
///
/// # Example
///
/// ```rust,no_run
/// # use std::path::Path;
/// # use anyhow::Result;
/// # async fn example() -> Result<()> {
/// use cargo_version_info::commands::bump::hooks::run_hook;
///
/// run_hook(
///     "./scripts/sync-version.sh {{version}}",
///     "1.2.3",
///     Path::new("."),
/// )?;
/// # Ok(())
/// # }
/// ```
pub fn run_hook(command: &str, version: &str, working_dir: &Path) -> Result<()> {
    // Replace {{version}} placeholder with actual version
    let expanded = command.replace("{{version}}", version);

    // Execute the command via platform-appropriate shell
    #[cfg(unix)]
    let status = std::process::Command::new("sh")
        .args(["-c", &expanded])
        .current_dir(working_dir)
        .status()
        .with_context(|| format!("Failed to run hook: {}", command))?;

    #[cfg(windows)]
    let status = std::process::Command::new("cmd")
        .args(["/C", &expanded])
        .current_dir(working_dir)
        .status()
        .with_context(|| format!("Failed to run hook: {}", command))?;

    if !status.success() {
        let exit_code = status
            .code()
            .map_or("unknown".to_string(), |c| c.to_string());
        anyhow::bail!("Hook failed with exit code {}: {}", exit_code, command);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(unix)]
    async fn test_run_hook_success() {
        let dir = async_fs_io::TempDir::create(std::env::temp_dir())
            .await
            .unwrap();
        run_hook("true", "1.0.0", dir.path()).unwrap();
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_run_hook_failure() {
        let dir = async_fs_io::TempDir::create(std::env::temp_dir())
            .await
            .unwrap();
        let result = run_hook("false", "1.0.0", dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exit code"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_run_hook_version_substitution() {
        let dir = async_fs_io::TempDir::create(std::env::temp_dir())
            .await
            .unwrap();
        let output_file = dir.path().join("version.txt");

        // Create a hook that writes the version to a file
        let command = format!("echo '{{{{version}}}}' > {}", output_file.display());
        run_hook(&command, "2.3.4", dir.path()).unwrap();

        let content = async_fs_io::read_string_bounded(&output_file, 64 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(content.trim(), "2.3.4");
    }

    #[tokio::test]
    async fn test_version_info_config_default() {
        let config = VersionInfoConfig::default();
        assert!(config.pre_bump_hooks.is_empty());
        assert!(config.post_bump_hooks.is_empty());
        assert!(config.additional_files.is_empty());
    }

    #[tokio::test]
    async fn test_version_info_config_deserialize() {
        let json = serde_json::json!({
            "pre_bump_hooks": ["./scripts/pre.sh {{version}}"],
            "post_bump_hooks": ["./scripts/post.sh"],
            "additional_files": ["package.json"]
        });

        let config: VersionInfoConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.pre_bump_hooks.len(), 1);
        assert_eq!(config.post_bump_hooks.len(), 1);
        assert_eq!(config.additional_files.len(), 1);
        assert!(config.pre_bump_hooks[0].contains("{{version}}"));
    }

    #[tokio::test]
    async fn test_version_info_config_partial() {
        // Test that missing fields default to empty
        let json = serde_json::json!({
            "pre_bump_hooks": ["./scripts/pre.sh"]
        });

        let config: VersionInfoConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.pre_bump_hooks.len(), 1);
        assert!(config.post_bump_hooks.is_empty());
        assert!(config.additional_files.is_empty());
    }
}
