//! Version bumping command.
//!
//! This module implements the `cargo version-info bump` subcommand, which
//! intelligently bumps version numbers in Cargo.toml and creates focused
//! git commits containing only the version changes.
//!
//! # Overview
//!
//! The bump command solves a common problem in version management: how to
//! commit version changes without including other uncommitted work. Unlike
//! tools like `cocogitto` (which also manages tags and changelogs), this
//! command focuses solely on version bumping and commit creation.
//!
//! # Architecture
//!
//! The module is split into focused sub-modules:
//!
//! - [`args`] - Command-line argument definitions
//! - [`version_update`] - TOML file manipulation
//! - [`index`] - Git index (staging area) operations
//! - [`tree`] - Git tree building from index
//! - [`commit`] - Commit orchestration and creation
//!
//! # Usage Examples
//!
//! ```bash
//! # Bump patch version (most common)
//! cargo version-info bump --patch
//!
//! # Bump minor version for new features
//! cargo version-info bump --minor
//!
//! # Bump major version for breaking changes
//! cargo version-info bump --major
//!
//! # Set specific version
//! cargo version-info bump --version 2.0.0
//!
//! # Auto-suggest from GitHub releases
//! cargo version-info bump --auto --github-token $TOKEN
//!
//! # Update but don't commit
//! cargo version-info bump --patch --no-commit
//! ```
//!
//! # Workflow
//!
//! 1. **Calculate Target Version**
//!    - From explicit `--version` flag
//!    - From GitHub API (`--auto`)
//!    - From semantic version increment (`--major`, `--minor`, `--patch`)
//!
//! 2. **Update Cargo.toml**
//!    - Parse TOML while preserving formatting
//!    - Update version field
//!    - Write back to disk
//!
//! 3. **Create Commit** (unless `--no-commit`)
//!    - Verify version changes
//!    - Stage only the modified file
//!    - Build git tree from staged files
//!    - Create commit object
//!    - Update HEAD reference
//!
//! # Design Philosophy
//!
//! ## No Tags
//!
//! Unlike `cog bump`, this command does NOT create git tags. Tag creation
//! is left to CI/CD pipelines which can:
//! - Run tests before tagging
//! - Include release metadata
//! - Trigger deployment workflows
//! - Handle tag signing
//!
//! ## Selective Staging
//!
//! The command stages only the version changes, leaving other uncommitted
//! work untouched. This prevents accidental inclusion of WIP code in version
//! bump commits.
//!
//! ## Pure Rust Git Operations
//!
//! All git operations use `gix` (gitoxide) instead of shelling out to the
//! git binary. This provides:
//! - Better type safety
//! - No process spawning overhead
//! - Consistent error handling
//! - Easier testing
//!
//! # Implementation Notes
//!
//! ## Conventional Commits
//!
//! Commit messages follow the conventional commits format:
//! ```text
//! chore(version): bump X.Y.Z -> X.Y.Z
//! ```
//!
//! The `chore` type indicates this is a maintenance task, not a feature or fix.
//!
//! ## Workspace Support
//!
//! The command handles both:
//! - Regular crates with `[package] version`
//! - Workspace members with `[workspace.package] version`
//!
//! ## Error Handling
//!
//! All operations use `anyhow::Result` for consistent error handling with
//! context. Errors are bubbled up with descriptive messages about what failed
//! and why.

pub mod args;
pub mod commit;
pub mod diff;
pub mod hooks;
pub mod index;
pub mod readme_update;
pub mod signing;
pub mod tree;
pub mod version_update;

#[cfg(test)]
mod tests;

// Re-export public API
use anyhow::{
    Context,
    Result,
};
pub use args::BumpArgs;
use async_fs_io::{
    read_string_bounded,
    try_exists,
    write_bytes,
};
use cargo_plugin_utils::common::{
    find_package,
    get_owner_repo,
};

use crate::github;
use crate::version::{
    format_version,
    increment_major,
    increment_minor,
    increment_patch,
    parse_version,
};

/// Bump the version in Cargo.toml and commit only version-related changes.
///
/// This is the main entry point for the bump command. It orchestrates the
/// entire version bump process from calculation through commit.
///
/// # Process
///
/// 1. **Read Current Version**
///    - Use cargo_metadata to parse Cargo.toml
///    - Extract current version from package metadata
///
/// 2. **Calculate Target Version**
///    - Manual: Use `--version` argument directly
///    - Auto: Query GitHub API for latest release and suggest next
///    - Increment: Parse current version and apply semantic version rules
///
/// 3. **Update Files**
///    - Modify Cargo.toml with new version
///    - Preserve all formatting and comments
///
/// 4. **Create Commit** (unless `--no-commit`)
///    - Stage only the version changes
///    - Build tree from staged index
///    - Create commit object with conventional message
///    - Update HEAD to new commit
///
/// # Arguments
///
/// * `args` - Parsed command-line arguments (see [`BumpArgs`])
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns an error if:
/// - Cargo.toml cannot be read or parsed
/// - Current version cannot be determined
/// - Target version calculation fails
/// - File updates fail
/// - Git operations fail (when committing)
/// - Current version equals target version (nothing to bump)
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     BumpArgs,
///     bump,
/// };
/// use clap::Parser;
///
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// // Parse command-line arguments
/// let args = BumpArgs::parse_from(&["cargo", "version-info", "bump", "--patch"]);
///
/// // Execute the bump
/// bump(args).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Version Calculation
///
/// ## Semantic Versioning
///
/// Versions follow SemVer (MAJOR.MINOR.PATCH):
/// - MAJOR: Breaking changes (resets MINOR and PATCH to 0)
/// - MINOR: New features (resets PATCH to 0)
/// - PATCH: Bug fixes
///
/// ## Auto Mode
///
/// The `--auto` flag queries the GitHub Releases API to find the latest
/// published version and suggests the next appropriate version. This is
/// useful in CI/CD pipelines where you want automated version suggestions.
///
/// # Commit Format
///
/// Commits use the conventional commits format:
/// ```text
/// chore(version): bump 0.1.0 -> 0.2.0
/// ```
///
/// This format:
/// - Is machine-parseable for changelog generation
/// - Clearly indicates the type of change
/// - Includes both old and new versions for context
///
/// # No-Commit Mode
///
/// The `--no-commit` flag allows updating the version without creating a
/// commit. This is useful when:
/// - You want to review changes first
/// - You're making multiple related changes
/// - You prefer manual commit control
pub async fn bump(args: BumpArgs) -> Result<()> {
    use commit::{
        AdditionalFile,
        FileType,
    };

    let mut logger = cargo_plugin_utils::logger::Logger::new();

    // Step 1: Get current version and package info from Cargo.toml
    logger.status("Reading", "current version");
    let package = find_package(args.manifest_path.as_deref())?;
    let current_version = package.version.to_string();
    let package_name = package.name.clone();

    // Load hook configuration from [package.metadata.version-info]
    let hook_config = hooks::VersionInfoConfig::from_package(&package);
    logger.finish();

    // Step 2: Calculate target version based on command args
    logger.status("Calculating", "target version");
    let target_version = calculate_target_version(&args, &current_version).await?;
    logger.finish();

    // Step 3: Verify version is changing
    if current_version == target_version {
        anyhow::bail!(
            "Current version ({}) is already the target version. Nothing to bump.",
            current_version
        );
    }

    logger.print_message(&format!(
        "Bumping version: {} -> {}",
        current_version, target_version
    ));

    // Step 4: Update Cargo.toml
    logger.status("Updating", "Cargo.toml");
    let manifest_path = args
        .manifest_path
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("./Cargo.toml"));
    version_update::update_cargo_toml_version(manifest_path, &current_version, &target_version)
        .await?;
    logger.finish();

    // Get the directory containing Cargo.toml for other files
    let manifest_dir = manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    // Step 5: Run pre-bump hooks
    //
    // Hooks run after Cargo.toml version update and before other derived files
    // are generated (Cargo.lock, README.md). This allows hooks to update
    // workspace metadata that influences lockfile resolution.
    for hook in &hook_config.pre_bump_hooks {
        logger.status("Running", &format!("hook: {}", hook));
        hooks::run_hook(hook, &target_version, manifest_dir)?;
        logger.finish();
    }

    // Step 6: Update Cargo.lock (unless --no-lock)
    // First, capture the HEAD content of Cargo.lock for selective staging
    let cargo_lock_path = manifest_dir.join("Cargo.lock");
    let cargo_lock_head_content = if !args.no_lock && try_exists(&cargo_lock_path).await? {
        // Read HEAD content using gix
        get_file_head_content(manifest_path, &cargo_lock_path).ok()
    } else {
        None
    };

    if !args.no_lock {
        logger.status("Updating", "Cargo.lock");
        let status = std::process::Command::new("cargo")
            .args(["update", "--workspace"])
            .current_dir(manifest_dir)
            .status()
            .context("Failed to run cargo update")?;

        if !status.success() {
            anyhow::bail!("cargo update --workspace failed");
        }
        logger.finish();
    }

    // Step 7: Update README.md (unless --no-readme)
    // First, capture HEAD content for selective staging
    let readme_path = manifest_dir.join("README.md");
    let readme_head_content = if !args.no_readme && try_exists(&readme_path).await? {
        get_file_head_content(manifest_path, &readme_path).ok()
    } else {
        None
    };

    let readme_update = if !args.no_readme {
        logger.status("Checking", "README.md");
        let result = readme_update::update_readme_file(
            &readme_path,
            &package_name,
            &current_version,
            &target_version,
        )
        .await?;
        logger.finish();

        if let Some(ref update) = result
            && update.modified
        {
            // Write the updated README
            write_bytes(&readme_path, update.content.as_bytes())
                .await
                .with_context(|| format!("Failed to write {}", readme_path.display()))?;
            logger.print_message("  Updated version in README.md");
        }
        result
    } else {
        None
    };

    // Step 8: Commit changes (unless --no-commit)
    if !args.no_commit {
        logger.status("Committing", "version changes");

        // Collect additional files to commit (besides Cargo.toml which is handled
        // specially)
        let mut additional_files: Vec<AdditionalFile> = Vec::new();

        // Include Cargo.lock if it was updated
        if !args.no_lock && try_exists(&cargo_lock_path).await? {
            let cargo_lock_content = read_string_bounded(&cargo_lock_path, 64 * 1024 * 1024)
                .await
                .with_context(|| format!("Failed to read {}", cargo_lock_path.display()))?;
            additional_files.push(AdditionalFile {
                path: cargo_lock_path,
                working_content: cargo_lock_content,
                head_content: cargo_lock_head_content,
                file_type: FileType::CargoLock,
            });
        }

        // Include README.md if it was modified
        if let Some(update) = readme_update
            && update.modified
        {
            additional_files.push(AdditionalFile {
                path: readme_path,
                working_content: update.content,
                head_content: readme_head_content,
                file_type: FileType::Readme,
            });
        }

        // Include additional files from hook configuration
        for file_path in &hook_config.additional_files {
            let path = manifest_dir.join(file_path);
            if try_exists(&path).await? {
                let content = read_string_bounded(&path, 16 * 1024 * 1024)
                    .await
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                let head_content = get_file_head_content(manifest_path, &path).ok();
                additional_files.push(AdditionalFile {
                    path,
                    working_content: content,
                    head_content,
                    file_type: FileType::Other,
                });
            } else {
                logger.print_message(&format!(
                    "⚠️  Additional file not found: {}",
                    path.display()
                ));
            }
        }

        // Commit Cargo.toml (with selective staging) plus additional files
        commit::commit_version_changes_with_files(
            manifest_path,
            &package_name,
            &current_version,
            &target_version,
            &additional_files,
        )
        .await?;
        logger.finish();

        let file_count = additional_files.len() + 1; // +1 for Cargo.toml
        logger.print_message(&format!(
            "✓ Committed version bump: {} -> {} ({} file{})",
            current_version,
            target_version,
            file_count,
            if file_count == 1 { "" } else { "s" }
        ));

        // Step 9: Run post-bump hooks (only after commit)
        for hook in &hook_config.post_bump_hooks {
            logger.status("Running", &format!("hook: {}", hook));
            hooks::run_hook(hook, &target_version, manifest_dir)?;
            logger.finish();
        }
    } else {
        logger.print_message(&format!(
            "✓ Updated version to {} (not committed)",
            target_version
        ));
    }

    Ok(())
}

/// Calculate the target version based on command arguments.
///
/// This function implements the version selection logic for all supported
/// modes:
/// - Manual version specification
/// - Automatic suggestion from GitHub
/// - Semantic version increments (major/minor/patch)
///
/// # Arguments
///
/// * `args` - Command-line arguments containing version selection flags
/// * `current_version` - The current version string (e.g., "0.1.0")
///
/// # Returns
///
/// Returns the calculated target version as a string.
///
/// # Errors
///
/// Returns an error if:
/// - GitHub API query fails (in auto mode)
/// - Version parsing fails
/// - Network requests fail
async fn calculate_target_version(args: &BumpArgs, current_version: &str) -> Result<String> {
    if let Some(version) = &args.version {
        // Manual version specified
        Ok(version.trim().to_string())
    } else if args.auto {
        // Auto-suggest from GitHub releases
        let (owner, repo) = get_owner_repo(args.owner.clone(), args.repo.clone())?;
        let github_token = args.github_token.as_deref();
        let (_latest, next) = github::calculate_next_version(&owner, &repo, github_token).await?;
        Ok(next)
    } else {
        // Semantic version increment
        let (major, minor, patch) = parse_version(current_version)?;
        let (new_major, new_minor, new_patch) = if args.major {
            increment_major(major, minor, patch)
        } else if args.minor {
            increment_minor(major, minor, patch)
        } else if args.patch {
            increment_patch(major, minor, patch)
        } else {
            // Default to patch if no flag specified
            increment_patch(major, minor, patch)
        };
        Ok(format_version(new_major, new_minor, new_patch))
    }
}

/// Get the content of a file from HEAD commit.
///
/// This is used for selective staging - we need the HEAD content to compare
/// against the working directory content and filter out non-version changes.
///
/// # Arguments
///
/// * `manifest_path` - Path to Cargo.toml (used to discover the git repo)
/// * `file_path` - Path to the file to read from HEAD
///
/// # Returns
///
/// Returns the file content from HEAD as a String.
///
/// # Errors
///
/// Returns an error if:
/// - Not in a git repository
/// - File doesn't exist in HEAD
/// - Git operations fail
fn get_file_head_content(
    manifest_path: &std::path::Path,
    file_path: &std::path::Path,
) -> Result<String> {
    use bstr::ByteSlice;

    // Discover git repository
    let repo = gix::discover(
        manifest_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    )
    .context("Not in a git repository")?;

    // Calculate relative path from repository root
    let repo_path = repo.path().parent().context("Invalid repository path")?;
    let relative_path = file_path
        .strip_prefix(repo_path)
        .or_else(|_| file_path.strip_prefix("."))
        .unwrap_or(file_path);

    // Get HEAD commit
    let head = repo.head().context("Failed to read HEAD")?;
    let head_commit_id = head.id().context("HEAD does not point to a commit")?;
    let head_commit = repo
        .find_object(head_commit_id)
        .context("Failed to find HEAD commit")?
        .try_into_commit()
        .context("HEAD is not a commit")?;

    // Get tree from HEAD
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    // Lookup the file in the tree
    let entry = head_tree
        .lookup_entry_by_path(relative_path)
        .context("Failed to lookup file in HEAD tree")?
        .with_context(|| format!("File {} does not exist in HEAD", relative_path.display()))?;

    // Read blob content
    let blob = entry
        .object()
        .context("Failed to get blob from tree entry")?
        .try_into_blob()
        .context("Tree entry is not a blob")?;

    Ok(blob.data.to_str_lossy().into_owned())
}
