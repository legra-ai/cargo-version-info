//! Cargo subcommand for unified version management.
//!
//! This tool provides a single source of truth for version operations:
//! - Calculate next version from GitHub releases
//! - Read current version from Cargo.toml
//! - Compare versions
//! - Generate dev versions from git SHA
//! - Generate tag names
//!
//! Replaces scattered version logic in GitHub Actions, bash scripts, and Rust
//! code.

use std::path::PathBuf;

use anyhow::Result;
use async_fs_io::{
    metadata,
    try_exists,
};
use cargo_version_info::commands;
use cargo_version_info::commands::{
    BadgeArgs,
    BuildVersionArgs,
    BumpArgs,
    ChangedArgs,
    ChangelogArgs,
    CompareArgs,
    CurrentArgs,
    DevArgs,
    DioxusArgs,
    LatestArgs,
    NextArgs,
    PostBumpHookArgs,
    PrLogArgs,
    PreBumpHookArgs,
    ReleasePageArgs,
    RustToolchainArgs,
    TagArgs,
    UpdateReadmeArgs,
};
use clap::{
    ArgAction,
    CommandFactory,
    Parser,
    Subcommand,
};

#[derive(Parser, Debug)]
#[command(
    bin_name = "cargo",
    disable_version_flag = true,
    arg_required_else_help = false
)]
struct CargoArgs {
    /// Compute version for the current repo (same logic as build-version).
    #[arg(long = "tool-version", short = 'T')]
    tool_version_flag: bool,

    #[command(subcommand)]
    subcmd: Option<TopCommand>,
}

#[derive(Subcommand, Debug)]
enum TopCommand {
    /// Unified version management for Rust projects
    #[command(name = "version-info")]
    VersionInfo(VersionInfoCli),
}

#[derive(Parser, Debug)]
#[command(
    disable_version_flag = true,
    subcommand_required = false,
    arg_required_else_help = false
)]
struct VersionInfoCli {
    /// Show computed version (same as `cargo version-info build-version`).
    #[arg(long = "version", short = 'V', action = ArgAction::SetTrue)]
    version_flag: bool,

    #[command(subcommand)]
    command: Option<VersionInfoCommand>,

    /// Capture trailing args after `--` (e.g., `--version`).
    #[arg(trailing_var_arg = true, hide = true)]
    passthrough: Vec<String>,
}

#[derive(Parser, Debug)]
enum VersionInfoCommand {
    /// Calculate next patch version from latest GitHub release
    #[command(name = "next")]
    Next(NextArgs),
    /// Get current version from Cargo.toml
    #[command(name = "current")]
    Current(CurrentArgs),
    /// Get latest GitHub release version
    #[command(name = "latest")]
    Latest(LatestArgs),
    /// Generate dev version from git SHA
    #[command(name = "dev")]
    Dev(DevArgs),
    /// Generate tag name (e.g., v0.0.1)
    #[command(name = "tag")]
    Tag(TagArgs),
    /// Compare two versions
    #[command(name = "compare")]
    Compare(CompareArgs),
    /// Get Rust toolchain version from .rust-toolchain.toml
    #[command(name = "rust-toolchain")]
    RustToolchain(RustToolchainArgs),
    /// Get Dioxus version from Cargo.toml
    #[command(name = "dioxus")]
    Dioxus(DioxusArgs),
    /// Determine build version with priority logic
    #[command(name = "build-version")]
    BuildVersion(BuildVersionArgs),
    /// Check if Cargo.toml version changed since last git tag
    #[command(name = "changed")]
    Changed(ChangedArgs),
    /// Bump version in Cargo.toml and commit changes (does not create tags)
    #[command(name = "bump")]
    Bump(BumpArgs),
    /// Pre-bump hook for cog integration (verifies state before bumping)
    #[command(name = "pre-bump-hook")]
    PreBumpHook(PreBumpHookArgs),
    /// Post-bump hook for cog integration (verifies bump succeeded)
    #[command(name = "post-bump-hook")]
    PostBumpHook(PostBumpHookArgs),
    /// Generate changelog from conventional commits
    #[command(name = "changelog")]
    Changelog(ChangelogArgs),
    /// Generate PR log from merged pull requests
    #[command(name = "pr-log")]
    PrLog(PrLogArgs),
    /// Generate complete release page with badges, PR log, and changelog
    #[command(name = "release-page")]
    ReleasePage(ReleasePageArgs),
    /// Generate badges for quality metrics
    #[command(name = "badge")]
    Badge(BadgeArgs),
    /// Update README with badges
    #[command(name = "update-readme")]
    UpdateReadme(UpdateReadmeArgs),
    /// Compute effective version (same as --version)
    #[command(name = "version")]
    Version,
}

/// Check if any .env* files exist in the current directory.
async fn has_env_files() -> anyhow::Result<bool> {
    let current_dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(error) => return Err(error.into()),
    };

    // Check for common .env* file patterns
    let patterns = [".env", ".env.local", ".env.prod", ".env.dev", ".env.test"];

    for pattern in &patterns {
        let path = current_dir.join(pattern);
        if try_exists(&path).await? && !metadata(&path).await?.is_directory {
            return Ok(true);
        }
    }

    // Also check for .env.{USER} pattern
    if let Ok(user) = std::env::var("USER") {
        let user_env = format!(".env.{}", user);
        let path = current_dir.join(user_env);
        if try_exists(&path).await? && !metadata(&path).await?.is_directory {
            return Ok(true);
        }
    }

    Ok(false)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env* files using dotenvage
    // This allows cargo-version-info to access encrypted secrets like GITHUB_TOKEN
    // stored in .env.local files protected by dotenvage
    // Only attempt to load if .env* files exist to avoid unnecessary warnings
    if has_env_files().await?
        && let Err(e) = dotenvage::EnvLoader::new().and_then(|loader| loader.load())
    {
        eprintln!("Warning: Failed to load/decrypt env files: {}", e);
        eprintln!("Continuing with existing environment variables...");
    }

    let args = CargoArgs::parse();

    if args.tool_version_flag {
        return commands::build_version_for_repo(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).await;
    }

    if let Some(TopCommand::VersionInfo(cli)) = args.subcmd {
        if cli.version_flag {
            return commands::build_version_default().await;
        }

        if let Some(command) = cli.command {
            return match command {
                VersionInfoCommand::Next(args) => commands::next(args).await,
                VersionInfoCommand::Current(args) => commands::current(args).await,
                VersionInfoCommand::Latest(args) => commands::latest(args).await,
                VersionInfoCommand::Dev(args) => commands::dev(args).await,
                VersionInfoCommand::Tag(args) => commands::tag(args).await,
                VersionInfoCommand::Compare(args) => commands::compare(args).await,
                VersionInfoCommand::RustToolchain(args) => commands::rust_toolchain(args).await,
                VersionInfoCommand::Dioxus(args) => commands::dioxus(args).await,
                VersionInfoCommand::BuildVersion(args) => commands::build_version(args).await,
                VersionInfoCommand::Changed(args) => commands::changed(args).await,
                VersionInfoCommand::Bump(args) => commands::bump(args).await,
                VersionInfoCommand::PreBumpHook(args) => commands::pre_bump_hook(args).await,
                VersionInfoCommand::PostBumpHook(args) => commands::post_bump_hook(args).await,
                VersionInfoCommand::Changelog(args) => commands::changelog(args).await,
                VersionInfoCommand::PrLog(args) => commands::pr_log(args).await,
                VersionInfoCommand::ReleasePage(args) => commands::release_page(args).await,
                VersionInfoCommand::Badge(args) => commands::badge(args).await,
                VersionInfoCommand::UpdateReadme(args) => commands::update_readme(args).await,
                VersionInfoCommand::Version => commands::build_version_default().await,
            };
        }

        if cli
            .passthrough
            .iter()
            .any(|arg| arg == "--version" || arg == "-V")
        {
            return commands::build_version_default().await;
        }
        if cli
            .passthrough
            .iter()
            .any(|arg| arg == "--tool-version" || arg == "-T")
        {
            return commands::build_version_for_repo(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
                .await;
        }

        // No inner command: show help
        VersionInfoCli::command().print_help()?;
        println!();
        return Ok(());
    }

    // No subcommand: show help
    CargoArgs::command().print_help()?;
    println!();
    Ok(())
}
