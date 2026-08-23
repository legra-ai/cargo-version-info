//! Generate badges for quality metrics.
//!
//! This command generates various badges (tests, coverage, quality metrics)
//! that can be included in markdown documents.
//!
//! # Examples
//!
//! ```bash
//! # Generate all badges
//! cargo version-info badge all
//!
//! # Generate docs.rs badge (only if published)
//! cargo version-info badge rustdocs
//!
//! # Generate crates.io badge (only if published)
//! cargo version-info badge cratesio
//!
//! # Generate license badge
//! cargo version-info badge license
//!
//! # Generate Rust edition badge
//! cargo version-info badge rust-edition
//!
//! # Generate runtime badge
//! cargo version-info badge runtime
//!
//! # Generate framework badge
//! cargo version-info badge framework
//!
//! # Generate platform badge
//! cargo version-info badge platform
//!
//! # Generate ADRs badge
//! cargo version-info badge ADRs
//!
//! # Generate coverage badge (requires cargo-llvm-cov)
//! cargo version-info badge coverage
//!
//! # Generate number of tests badge
//! cargo version-info badge number-of-tests
//!
//! # Use heuristics instead of network requests
//! cargo version-info badge all --no-network
//! cargo version-info badge rustdocs --no-network
//! ```

mod adrs;
mod all;
mod common;
mod coverage;
mod crates_io;
mod docs_rs;
mod framework;
mod license;
mod number_of_tests;
mod platform;
mod runtime;
mod rust_edition;

use std::io::Write;

// Re-export for use by other commands (like release_page)
pub use all::badge_all;
use anyhow::{
    Context,
    Result,
};
use async_fs_io::canonicalize;
use clap::{
    Parser,
    Subcommand,
};

/// Arguments for the `badge` command.
#[derive(Parser, Debug)]
pub struct BadgeArgs {
    /// Skip network requests and use heuristics to guess if crate is published.
    ///
    /// When set, checks:
    /// - `publish` field in Cargo.toml
    /// - Whether any GitHub workflow files contain "cargo publish"
    /// - Whether LICENSE file exists
    #[arg(long)]
    pub no_network: bool,

    /// The badge subcommand to execute.
    #[command(subcommand)]
    pub subcommand: BadgeSubcommand,
}

/// Subcommands for the badge command.
#[derive(Subcommand, Debug)]
pub enum BadgeSubcommand {
    /// Generate all badges (including rustdocs and cratesio if published).
    All,
    /// Show the docs.rs badge if the project is published there, otherwise no
    /// output.
    Rustdocs,
    /// Show the crates.io badge if the project is published there, otherwise no
    /// output.
    Cratesio,
    /// Show the license badge.
    License,
    /// Show the Rust edition badge.
    #[command(name = "rust-edition")]
    RustEdition,
    /// Show the runtime badge (Tokio, etc.).
    Runtime,
    /// Show the framework badge (Axum, etc.).
    Framework,
    /// Show the platform badge (Fly.io, Vercel, etc.).
    Platform,
    /// Show the ADRs badge if docs/adr/ exists.
    ADRs,
    /// Show the test coverage badge (requires cargo-llvm-cov).
    Coverage,
    /// Show the number of tests badge.
    #[command(name = "number-of-tests")]
    NumberOfTests,
}

/// Generate badges for quality metrics.
pub async fn badge(args: BadgeArgs) -> Result<()> {
    badge_async(args).await
}

/// Async entry point for badge generation.
async fn badge_async(args: BadgeArgs) -> Result<()> {
    // Create logger - status messages go to stderr, badges to stdout
    let mut logger = cargo_plugin_utils::logger::Logger::new();

    // Detect package from Cargo's context (working directory when
    // --manifest-path is used)
    logger.status("Checking", "package metadata");
    let package = find_package().await?;

    // Buffer all badge output to avoid mixing with stderr status lines
    let mut buffer = Vec::new();

    // Drop the initial logger - each badge function creates its own
    drop(logger);

    match args.subcommand {
        BadgeSubcommand::All => {
            // Each badge function manages its own status logging via Drop
            docs_rs::badge_rustdocs(&mut buffer, &package, args.no_network).await?;
            crates_io::badge_cratesio(&mut buffer, &package, args.no_network).await?;
            license::badge_license(&mut buffer, &package).await?;
            rust_edition::badge_rust_edition(&mut buffer, &package).await?;
            runtime::badge_runtime(&mut buffer, &package).await?;
            framework::badge_framework(&mut buffer, &package).await?;
            platform::badge_platform(&mut buffer, &package).await?;
            adrs::badge_adrs(&mut buffer, &package).await?;
            coverage::badge_coverage(&mut buffer, &package).await?;
            number_of_tests::badge_number_of_tests(&mut buffer, &package).await?;

            Ok(())
        }
        BadgeSubcommand::Rustdocs => {
            docs_rs::badge_rustdocs(&mut buffer, &package, args.no_network).await
        }
        BadgeSubcommand::Cratesio => {
            crates_io::badge_cratesio(&mut buffer, &package, args.no_network).await
        }
        BadgeSubcommand::License => license::badge_license(&mut buffer, &package).await,
        BadgeSubcommand::RustEdition => {
            rust_edition::badge_rust_edition(&mut buffer, &package).await
        }
        BadgeSubcommand::Runtime => runtime::badge_runtime(&mut buffer, &package).await,
        BadgeSubcommand::Framework => framework::badge_framework(&mut buffer, &package).await,
        BadgeSubcommand::Platform => platform::badge_platform(&mut buffer, &package).await,
        BadgeSubcommand::ADRs => adrs::badge_adrs(&mut buffer, &package).await,
        BadgeSubcommand::Coverage => coverage::badge_coverage(&mut buffer, &package).await,
        BadgeSubcommand::NumberOfTests => {
            number_of_tests::badge_number_of_tests(&mut buffer, &package).await
        }
    }?;

    // Now write all buffered output to stdout at once
    std::io::stdout().write_all(&buffer)?;

    Ok(())
}

/// Find the Cargo package using cargo_metadata.
///
/// This automatically respects Cargo's `--manifest-path` option when running
/// as a cargo subcommand.
///
/// Returns the package that corresponds to the current context, in order:
/// 1. Package whose directory matches the current working directory
/// 2. Package whose manifest path matches `current_dir/Cargo.toml`
/// 3. Root package (if workspace has a root package)
/// 4. First default-member (if workspace has default-members configured)
/// 5. Error if no package can be determined
pub async fn find_package() -> Result<cargo_metadata::Package> {
    use cargo_metadata::MetadataCommand;

    // Use cargo_metadata which automatically respects --manifest-path
    let metadata = tokio::task::spawn_blocking(|| MetadataCommand::new().exec())
        .await
        .context("Failed to spawn blocking task")?
        .context("Failed to get cargo metadata")?;

    // Try to find the package in the current working directory
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;

    // Canonicalize current directory and all package directories, then find match.
    let canonical_current_dir = canonicalize(&current_dir).await.ok();
    let mut packages_with_dirs = Vec::new();
    for package in &metadata.packages {
        let Some(package_dir) = package.manifest_path.as_std_path().parent() else {
            continue;
        };
        if let Ok(canonical_package_dir) = canonicalize(package_dir).await {
            packages_with_dirs.push((package.clone(), canonical_package_dir));
        }
    }

    // Try to match current directory with a package directory
    if let Some(ref canonical_current) = canonical_current_dir
        && let Some((pkg, _)) = packages_with_dirs
            .iter()
            .find(|(_, pkg_dir)| pkg_dir == canonical_current)
    {
        return Ok(pkg.clone());
    }

    // Also try matching the manifest path directly (for cases where Cargo.toml is
    // in current dir)
    let current_manifest = current_dir.join("Cargo.toml");
    let canonical_current_manifest = canonicalize(&current_manifest).await.ok();
    let mut packages_with_manifests = Vec::new();
    for package in &metadata.packages {
        if let Ok(canonical_manifest) = canonicalize(package.manifest_path.as_std_path()).await {
            packages_with_manifests.push((package.clone(), canonical_manifest));
        }
    }

    if let Some(ref canonical) = canonical_current_manifest
        && let Some((pkg, _)) = packages_with_manifests
            .iter()
            .find(|(_, pkg_path)| pkg_path == canonical)
    {
        return Ok(pkg.clone());
    }

    // Fallback to root package (workspace root or single package)
    if let Some(root_package) = metadata.root_package() {
        return Ok(root_package.clone());
    }

    // If we're in a workspace without a root package, check for default-members
    // This follows cargo's behavior: use default-members if available
    // workspace_default_members implements Deref<Target = [PackageId]>, so we can
    // use it as a slice It may not be available in older Cargo versions, so we
    // check if it's available first
    if metadata.workspace_default_members.is_available()
        && !metadata.workspace_default_members.is_empty()
        && let Some(first_default_id) = metadata.workspace_default_members.first()
        && let Some(default_package) = metadata
            .packages
            .iter()
            .find(|pkg| &pkg.id == first_default_id)
    {
        return Ok(default_package.clone());
    }

    // If no default-members, we need to be in a package directory
    anyhow::bail!(
        "No package found in current directory. Run this command from a package directory, \
         or use --manifest-path to specify a package."
    )
}
