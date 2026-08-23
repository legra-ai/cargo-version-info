//! Generate a complete release page combining badges, PR log, and changelog.
//!
//! This command combines multiple outputs into a single release page document
//! that can be used for GitHub releases or other documentation.
//!
//! # Examples
//!
//! ```bash
//! # Generate complete release page
//! cargo version-info release-page
//!
//! # Generate since specific tag
//! cargo version-info release-page --since-tag v0.1.0
//!
//! # Skip network requests for badges
//! cargo version-info release-page --no-network
//!
//! # Output to file
//! cargo version-info release-page --output RELEASE.md
//! ```

use std::io::Write;

use anyhow::{
    Context,
    Result,
};
use async_fs_io::write_bytes;
use clap::Parser;

/// Arguments for the `release-page` command.
#[derive(Parser, Debug)]
pub struct ReleasePageArgs {
    /// Tag to compare from (default: latest tag).
    #[arg(long)]
    pub since_tag: Option<String>,

    /// Generate changelog for a commit range (e.g., v0.1.0..v0.2.0).
    #[arg(long)]
    pub range: Option<String>,

    /// Version for this release (e.g., 0.1.0 or v0.1.0).
    ///
    /// This is used for the release page header. If not specified,
    /// the version from Cargo.toml will be used instead.
    #[arg(long)]
    pub for_version: Option<String>,

    /// Output file path (default: stdout).
    #[arg(short, long)]
    pub output: Option<String>,

    /// Skip network requests and use heuristics for badges.
    #[arg(long)]
    pub no_network: bool,

    /// GitHub repository owner (for linking commits/PRs).
    #[arg(long)]
    pub owner: Option<String>,

    /// GitHub repository name (for linking commits/PRs).
    #[arg(long)]
    pub repo: Option<String>,
}

/// Generate a complete release page.
pub async fn release_page(args: ReleasePageArgs) -> Result<()> {
    release_page_async(args).await
}

/// Async entry point for release page generation.
async fn release_page_async(args: ReleasePageArgs) -> Result<()> {
    // Create logger - status messages go to stderr, release page to stdout
    let mut logger = cargo_plugin_utils::logger::Logger::new();

    logger.status("Generating", "release page");

    // Find the package
    let package = super::badge::find_package().await?;

    // Prepare output buffer
    let mut output = Vec::new();

    // Section 1: Title and Badges
    logger.status("Generating", "badges");
    // Use for_version if provided, otherwise fall back to package version
    let version_display = if let Some(ref version) = args.for_version {
        // Normalize version to have v prefix for display
        if version.starts_with('v') || version.starts_with('V') {
            version.clone()
        } else {
            format!("v{}", version)
        }
    } else {
        format!("v{}", package.version)
    };
    writeln!(&mut output, "# {} {}\n", package.name, version_display)?;

    // Add description if available
    if let Some(description) = &package.description {
        writeln!(&mut output, "{}\n", description)?;
    }

    // Add repository link if available
    if let Some(repository) = &package.repository {
        if repository.starts_with("https://github.com/") {
            writeln!(&mut output, "[View on GitHub]({})\n", repository)?;
        } else if repository.starts_with("http") {
            writeln!(&mut output, "[View Repository]({})\n", repository)?;
        }
    }

    super::badge::badge_all(&mut output, &package, args.no_network).await?;
    writeln!(&mut output)?;

    // Section 2: PR Log (optional - skip if not available)
    logger.status("Generating", "PR log");
    match generate_pr_log(&mut output, &args).await {
        Ok(_) => {
            writeln!(&mut output)?;
        }
        Err(_) => {
            // PR log not implemented yet, skip silently
            logger.warning("Skipping", "PR log (not yet implemented)");
        }
    }

    // Section 3: Changelog
    logger.status("Generating", "changelog");
    writeln!(&mut output, "## What's Changed\n")?;
    generate_changelog(&mut output, &args)?;

    // Add full changelog link if we have repository info
    if let Some(repository) = &package.repository
        && repository.starts_with("https://github.com/")
    {
        if let Some(range) = &args.range {
            // Extract start and end tags from range (e.g., "v0.1.0..v0.2.0")
            let parts: Vec<&str> = range.split("..").collect();
            if parts.len() == 2 {
                let start_tag = parts[0].trim();
                let end_tag = parts[1].trim();
                writeln!(
                    &mut output,
                    "\n**Full Changelog**: [{}/compare/{}...{}]({}/compare/{}...{})\n",
                    repository, start_tag, end_tag, repository, start_tag, end_tag
                )?;
            }
        } else if let Some(tag) = &args.since_tag {
            writeln!(
                &mut output,
                "\n**Full Changelog**: [{}/compare/{}...HEAD]({}/compare/{}...HEAD)\n",
                repository, tag, repository, tag
            )?;
        }
    }

    logger.finish();

    // Write output to file or stdout
    if let Some(output_path) = args.output {
        write_bytes(&output_path, &output).await.map_err(|error| {
            anyhow::anyhow!("Failed to write release page to {}: {error}", output_path)
        })?;
        logger.status("Written", &output_path);
    } else {
        std::io::stdout().write_all(&output)?;
    }

    Ok(())
}

/// Generate PR log section (stub for now).
async fn generate_pr_log(_writer: &mut dyn Write, args: &ReleasePageArgs) -> Result<()> {
    // Build arguments for pr_log command
    let pr_log_args = crate::commands::PrLogArgs {
        since_tag: args.since_tag.clone(),
        output: None, // We handle output ourselves
        owner: args.owner.clone(),
        repo: args.repo.clone(),
    };

    // Call pr_log - currently returns an error as it's not implemented
    crate::commands::pr_log(pr_log_args).await?;

    Ok(())
}

/// Generate changelog section.
fn generate_changelog(writer: &mut dyn Write, args: &ReleasePageArgs) -> Result<()> {
    // Build arguments for changelog command
    let changelog_args = crate::commands::ChangelogArgs {
        at: args.since_tag.clone(),
        range: args.range.clone(),
        for_version: args.for_version.clone(), // Use same version as release page
        output: None,                          // We handle output ourselves
        owner: args.owner.clone(),
        repo: args.repo.clone(),
    };

    // Generate changelog to a temporary buffer so we can process it
    let mut changelog_buffer = Vec::new();
    crate::commands::changelog::generate_changelog_to_writer(
        &mut changelog_buffer,
        changelog_args,
    )?;

    // Convert buffer to string and remove the header if present
    let changelog_str =
        String::from_utf8(changelog_buffer).context("Changelog output is not valid UTF-8")?;

    // Remove the "# Changelog" or "# Changelog - <tag>" header since we already
    // have "## Changelog"
    let cleaned_changelog = if changelog_str.starts_with("# Changelog") {
        // Find the first double newline after the header
        if let Some(pos) = changelog_str.find("\n\n") {
            changelog_str[pos + 2..].to_string()
        } else {
            changelog_str
        }
    } else {
        changelog_str
    };

    // Write the cleaned changelog
    write!(writer, "{}", cleaned_changelog)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    async fn create_test_cargo_project() -> async_fs_io::TempDir {
        let dir = async_fs_io::TempDir::create(std::env::temp_dir())
            .await
            .unwrap();

        // Create Cargo.toml
        async_fs_io::write_bytes(
            dir.path().join("Cargo.toml"),
            br#"
[package]
name = "test-package"
version = "1.0.0"
description = "Test package"
repository = "https://github.com/test/repo"
"#,
        )
        .await
        .unwrap();

        // Create src directory with minimal lib.rs
        let src_dir = dir.path().join("src");
        async_fs_io::ensure_dir(&src_dir).await.unwrap();
        async_fs_io::write_bytes(src_dir.join("lib.rs"), b"// Test library\n")
            .await
            .unwrap();

        // Initialize git repo
        Command::new("git")
            .arg("init")
            .current_dir(dir.path())
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Create initial commit
        async_fs_io::write_bytes(dir.path().join("README.md"), b"# Test\n")
            .await
            .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "chore: initial commit"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        dir
    }

    #[tokio::test]
    #[serial_test::serial]
    #[cfg_attr(target_os = "windows", ignore)] // Skip on Windows due to subprocess/directory issues
    async fn test_release_page_with_for_version() {
        let _dir = create_test_cargo_project().await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();

        let output_file = async_fs_io::TempFile::create(std::env::temp_dir())
            .await
            .unwrap();
        let output_path = output_file.path().to_string_lossy().to_string();

        let args = ReleasePageArgs {
            since_tag: None,
            range: None,
            for_version: Some("v0.2.0".to_string()),
            output: Some(output_path.clone()),
            no_network: true, // Skip network requests for badges
            owner: Some("test".to_string()),
            repo: Some("repo".to_string()),
        };

        let result = release_page_async(args).await;
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok(), "Release page generation should succeed");

        // Verify the output contains the for_version
        let content = async_fs_io::read_string_bounded(output_path, 64 * 1024 * 1024)
            .await
            .unwrap();
        assert!(
            content.contains("test-package v0.2.0"),
            "Header should include for_version"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    #[cfg_attr(target_os = "windows", ignore)] // Skip on Windows due to subprocess/directory issues
    async fn test_release_page_with_for_version_no_v_prefix() {
        let _dir = create_test_cargo_project().await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();

        let output_file = async_fs_io::TempFile::create(std::env::temp_dir())
            .await
            .unwrap();
        let output_path = output_file.path().to_string_lossy().to_string();

        let args = ReleasePageArgs {
            since_tag: None,
            range: None,
            for_version: Some("0.2.0".to_string()), // No v prefix
            output: Some(output_path.clone()),
            no_network: true,
            owner: Some("test".to_string()),
            repo: Some("repo".to_string()),
        };

        let result = release_page_async(args).await;
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok(), "Release page generation should succeed");

        // Verify the output contains the normalized version
        let content = async_fs_io::read_string_bounded(output_path, 64 * 1024 * 1024)
            .await
            .unwrap();
        assert!(
            content.contains("test-package v0.2.0"),
            "Header should normalize version with v prefix"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    #[cfg_attr(target_os = "windows", ignore)] // Skip on Windows due to subprocess/directory issues
    async fn test_release_page_without_for_version_uses_package_version() {
        let _dir = create_test_cargo_project().await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();

        let args = ReleasePageArgs {
            since_tag: None,
            range: None,
            for_version: None, // Not specified - should use package version
            output: None,
            no_network: true,
            owner: Some("test".to_string()),
            repo: Some("repo".to_string()),
        };

        let output_file = async_fs_io::TempFile::create(std::env::temp_dir())
            .await
            .unwrap();
        let output_path = output_file.path().to_string_lossy().to_string();

        let mut args_with_output = args;
        args_with_output.output = Some(output_path.clone());

        let result = release_page_async(args_with_output).await;
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok(), "Release page generation should succeed");

        // Verify the output uses package version from Cargo.toml
        let content = async_fs_io::read_string_bounded(output_path, 64 * 1024 * 1024)
            .await
            .unwrap();
        assert!(
            content.contains("test-package v1.0.0"),
            "Header should use package version from Cargo.toml when for_version not specified"
        );
    }
}
