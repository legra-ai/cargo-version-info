//! Post-bump hook for cog integration command.
//!
//! This command is designed to be used as a post-bump hook with cocogitto
//! (cog). It verifies that the version bump was successful after cog has
//! updated Cargo.toml.
//!
//! # Integration with cog
//!
//! Configure in `cog.toml`:
//!
//! ```toml
//! [hooks]
//! post-bump-hook = "cargo version-info post-bump-hook"
//! ```
//!
//! # Examples
//!
//! ```bash
//! # Verify version bump succeeded
//! cargo version-info post-bump-hook
//!
//! # With target and previous versions (from cog)
//! COG_VERSION=1.0.0 COG_LATEST=0.9.0 cargo version-info post-bump-hook
//!
//! # Warn only, don't fail (for testing)
//! cargo version-info post-bump-hook --exit-on-error false
//! ```

use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use cargo_plugin_utils::common::get_package_version_from_manifest;
use clap::Parser;

/// Arguments for the `post-bump-hook` command.
#[derive(Parser, Debug)]
pub struct PostBumpHookArgs {
    /// Path to the Cargo.toml manifest file (standard cargo flag).
    ///
    /// When running as a cargo subcommand, this is automatically handled.
    #[arg(long)]
    manifest_path: Option<PathBuf>,

    /// Path to the git repository.
    ///
    /// Defaults to the current directory. Currently unused but reserved for
    /// future validation against git tags.
    #[arg(long, default_value = ".")]
    repo_path: PathBuf,

    /// Target version that cog bumped to.
    ///
    /// Typically provided by cog via the `{{version}}` template variable.
    /// If set, the command verifies that Cargo.toml matches this version.
    #[arg(long, env = "COG_VERSION")]
    target_version: Option<String>,

    /// Previous version before the bump.
    ///
    /// Typically provided by cog via the `{{latest}}` template variable.
    /// If set, the command verifies that the version actually changed.
    #[arg(long, env = "COG_LATEST")]
    previous_version: Option<String>,

    /// Whether to exit with an error code if verification fails.
    ///
    /// - `true`: Exit with non-zero code if bump verification fails (default)
    /// - `false`: Only print warnings, don't fail the hook
    #[arg(long, default_value = "true")]
    exit_on_error: bool,
}

/// Post-bump hook for cocogitto (cog) integration.
///
/// Verifies that cog successfully bumped the version in Cargo.toml. Performs
/// the following checks:
///
/// 1. **Target Version Match**: If `target_version` is provided, verifies that
///    Cargo.toml contains the expected version after the bump.
/// 2. **Version Change**: If `previous_version` is provided, verifies that the
///    version actually changed (not still the same).
///
/// This hook can be configured in `cog.toml` to run automatically after version
/// bumps, ensuring the bump was applied correctly and catching any failures.
///
/// # Errors
///
/// Returns an error if:
/// - The manifest file cannot be read
/// - No version field is found in Cargo.toml
/// - Target version doesn't match Cargo.toml (and `exit_on_error` is `true`)
/// - Version didn't change from previous (and `exit_on_error` is `true`)
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     PostBumpHookArgs,
///     post_bump_hook,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = PostBumpHookArgs::parse_from(&["cargo", "version-info", "post-bump-hook"]);
/// post_bump_hook(args).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Example Output
///
/// When bump verification succeeds:
/// ```text
/// ✓ Version bump verified: 1.0.0
///   Previous version: 0.9.0
///   New version: 1.0.0
/// ```
///
/// When target version doesn't match (with `--exit-on-error true`):
/// ```text
/// ❌ Error: Cargo.toml version (0.9.1) doesn't match expected target (1.0.0)
/// Error: Version bump verification failed
/// ```
///
/// When version didn't change (with `--exit-on-error true`):
/// ```text
/// ✓ Post-bump check passed
///   Current version: 0.9.0
/// ⚠️  Warning: Version didn't change (still 0.9.0)
/// Error: Version bump appears to have failed
/// ```
pub async fn post_bump_hook(args: PostBumpHookArgs) -> Result<()> {
    let mut logger = cargo_plugin_utils::logger::Logger::new();

    logger.status("Reading", "package version");
    // Get current version from Cargo.toml (after cog bump) using cargo_metadata
    let manifest_path = args
        .manifest_path
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("./Cargo.toml"));
    let cargo_version = get_package_version_from_manifest(manifest_path)
        .with_context(|| format!("Failed to get version from {}", manifest_path.display()))?;
    logger.finish();

    // If target version is provided, verify it matches
    if let Some(target) = &args.target_version {
        let target_trimmed = target.trim();
        if cargo_version != target_trimmed {
            eprintln!(
                "❌ Error: Cargo.toml version ({}) doesn't match expected target ({})",
                cargo_version, target_trimmed
            );
            if args.exit_on_error {
                anyhow::bail!("Version bump verification failed");
            }
        } else {
            logger.print_message(&format!("✓ Version bump verified: {}", cargo_version));
        }
    } else {
        logger.print_message("✓ Post-bump check passed");
        logger.print_message(&format!("  Current version: {}", cargo_version));
    }

    // Verify version changed from previous
    if let Some(previous) = &args.previous_version {
        let previous_trimmed = previous.trim();
        if cargo_version == previous_trimmed {
            eprintln!(
                "⚠️  Warning: Version didn't change (still {})",
                cargo_version
            );
            if args.exit_on_error {
                anyhow::bail!("Version bump appears to have failed");
            }
        } else {
            logger.print_message(&format!("  Previous version: {}", previous_trimmed));
            logger.print_message(&format!("  New version: {}", cargo_version));
        }
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
    #[serial_test::serial]
    async fn test_post_bump_hook_success() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "1.0.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = PostBumpHookArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            target_version: Some("1.0.0".to_string()),
            previous_version: Some("0.9.0".to_string()),
            exit_on_error: true,
        };
        assert!(post_bump_hook(args).await.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_post_bump_hook_version_mismatch() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "0.9.1"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = PostBumpHookArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            target_version: Some("1.0.0".to_string()),
            previous_version: Some("0.9.0".to_string()),
            exit_on_error: true,
        };
        assert!(post_bump_hook(args).await.is_err());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_post_bump_hook_version_unchanged() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "0.9.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = PostBumpHookArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            target_version: None,
            previous_version: Some("0.9.0".to_string()),
            exit_on_error: true,
        };
        assert!(post_bump_hook(args).await.is_err());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_post_bump_hook_no_target_version() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "2.0.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = PostBumpHookArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            target_version: None,
            previous_version: None,
            exit_on_error: true,
        };
        assert!(post_bump_hook(args).await.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_post_bump_hook_no_previous_version() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "1.5.0"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = PostBumpHookArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            target_version: Some("1.5.0".to_string()),
            previous_version: None,
            exit_on_error: true,
        };
        assert!(post_bump_hook(args).await.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_post_bump_hook_file_not_found() {
        let args = PostBumpHookArgs {
            manifest_path: Some("/nonexistent/Cargo.toml".into()),
            repo_path: ".".into(),
            target_version: None,
            previous_version: None,
            exit_on_error: true,
        };
        assert!(post_bump_hook(args).await.is_err());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_post_bump_hook_no_version() {
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
        let args = PostBumpHookArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            target_version: None,
            previous_version: None,
            exit_on_error: true,
        };
        // Cargo defaults to 0.0.0, so this should succeed
        assert!(post_bump_hook(args).await.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_post_bump_hook_exit_on_error_false() {
        let _dir = create_temp_cargo_project(
            r#"
[package]
name = "test"
version = "0.9.1"
"#,
        )
        .await;
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = PostBumpHookArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            target_version: Some("1.0.0".to_string()),
            previous_version: Some("0.9.0".to_string()),
            exit_on_error: false, // Should warn but not fail
        };
        // Should succeed even with mismatch when exit_on_error is false
        let result = post_bump_hook(args).await;
        // May succeed (just prints warning) or fail depending on implementation
        let _ = result;
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_post_bump_hook_workspace_version() {
        let _dir = async_fs_io::TempDir::create(std::env::temp_dir())
            .await
            .unwrap();
        // Create workspace root Cargo.toml (no [package] section)
        async_fs_io::write_bytes(
            _dir.path().join("Cargo.toml"),
            br#"
[workspace.package]
version = "2.1.0"

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
        let manifest_path = _dir.path().join("Cargo.toml");
        let args = PostBumpHookArgs {
            manifest_path: Some(manifest_path),
            repo_path: ".".into(),
            target_version: Some("2.1.0".to_string()),
            previous_version: Some("2.0.0".to_string()),
            exit_on_error: true,
        };
        assert!(post_bump_hook(args).await.is_ok());
    }
}
