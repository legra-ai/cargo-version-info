//! Generate test coverage badge.

use anyhow::{
    Context,
    Result,
};
use portable_pty::CommandBuilder;
use serde::{
    Deserialize,
    Serialize,
};

use super::common;

/// Show the test coverage badge.
pub async fn badge_coverage(
    writer: &mut dyn std::io::Write,
    package: &cargo_metadata::Package,
) -> Result<()> {
    let mut logger = cargo_plugin_utils::logger::Logger::new();
    // Use ephemeral status (cyan) for subprocess operations
    logger.status("Generating", "coverage badge");

    // Try to get coverage using cargo-llvm-cov
    let coverage = get_coverage_percentage(&mut logger, package).await?;

    if let Some(coverage) = coverage {
        // Determine badge color based on coverage percentage
        let color = if coverage >= 80 {
            "brightgreen"
        } else if coverage >= 60 {
            "green"
        } else if coverage >= 40 {
            "yellow"
        } else {
            "red"
        };

        let badge_url = format!(
            "https://img.shields.io/badge/coverage-{}%25-{}",
            coverage, color
        );

        // Determine link target: prefer GitHub repository, fallback to coverage
        // directory
        let link_target = if let Some(repo) = &package.repository {
            // Link to GitHub Actions if it's a GitHub repo, otherwise just the repo
            if repo.contains("github.com") {
                format!("{}/actions", repo)
            } else {
                repo.clone()
            }
        } else {
            "coverage/".to_string()
        };

        let badge_markdown = format!("[![Coverage]({})]({})", badge_url, link_target);
        writeln!(writer, "{}", badge_markdown)?;
    }

    Ok(())
}

/// Cache entry for coverage results.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CoverageCache {
    /// Package name
    package: String,
    /// Cache key (git commit hash or file mtime)
    cache_key: String,
    /// Coverage percentage
    coverage: u8,
}

/// Get test coverage percentage using cargo-llvm-cov.
/// Uses cache if available and valid.
async fn get_coverage_percentage(
    logger: &mut cargo_plugin_utils::logger::Logger,
    package: &cargo_metadata::Package,
) -> Result<Option<u8>> {
    // Try to load from cache first
    if let Some(cached) = load_coverage_cache(package).await? {
        let current_key = common::compute_cache_key(package).await?;
        if cached.cache_key == current_key && package.name == cached.package {
            return Ok(Some(cached.coverage));
        }
    }

    // Check if cargo-llvm-cov is available
    let version_output = cargo_plugin_utils::logger::run_subprocess(
        logger,
        || {
            let mut cmd = CommandBuilder::new("cargo");
            cmd.arg("llvm-cov");
            cmd.arg("--version");
            cmd
        },
        None,
    )
    .await?;

    if !version_output.success() {
        eprintln!(
            "Warning: cargo-llvm-cov is not installed. Install it with: cargo binstall cargo-llvm-cov (or cargo install cargo-llvm-cov)"
        );
        return Ok(None);
    }

    // Run cargo llvm-cov to get coverage
    let package_name = package.name.clone();
    let output = cargo_plugin_utils::logger::run_subprocess(
        logger,
        move || {
            let mut cmd = CommandBuilder::new("cargo");
            cmd.arg("llvm-cov");
            cmd.arg("--package");
            cmd.arg(package_name.as_str());
            cmd.arg("--summary-only");
            cmd.arg("--json");
            cmd
        },
        None,
    )
    .await?;

    if !output.success() {
        return Ok(None);
    }

    // Parse JSON output to extract coverage percentage
    let stdout = output
        .stdout_str()
        .context("Failed to parse cargo-llvm-cov output")?;

    // cargo-llvm-cov JSON format: {"data": [{"totals": {"lines": {"percent": 85.5},
    // ...}}], ...}
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout)
        && let Some(data) = json.get("data").and_then(|d| d.as_array())
        && let Some(first_data) = data.first()
        && let Some(percent) = first_data
            .get("totals")
            .and_then(|t| t.get("lines"))
            .and_then(|l| l.get("percent"))
            .and_then(|p| p.as_f64())
    {
        let coverage = percent.round() as u8;
        // Save to cache
        save_coverage_cache(package, coverage).await?;
        return Ok(Some(coverage));
    }

    Ok(None)
}

/// Load coverage from cache.
async fn load_coverage_cache(_package: &cargo_metadata::Package) -> Result<Option<CoverageCache>> {
    let cache_path = common::get_badge_cache_path("coverage").await?;

    if !async_fs_io::try_exists(&cache_path).await? {
        return Ok(None);
    }

    let contents = async_fs_io::read_string_bounded(&cache_path, 16 * 1024 * 1024)
        .await
        .context("Failed to read cache file")?;

    let cache: CoverageCache =
        serde_json::from_str(&contents).context("Failed to parse cache file")?;

    Ok(Some(cache))
}

/// Save coverage to cache.
async fn save_coverage_cache(package: &cargo_metadata::Package, coverage: u8) -> Result<()> {
    let cache_key = common::compute_cache_key(package).await?;
    let cache = CoverageCache {
        package: package.name.to_string(),
        cache_key,
        coverage,
    };

    let cache_path = common::get_badge_cache_path("coverage").await?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = cache_path.parent() {
        async_fs_io::ensure_dir(parent)
            .await
            .context("Failed to create cache directory")?;
    }

    let json = serde_json::to_string_pretty(&cache).context("Failed to serialize cache")?;

    async_fs_io::write_bytes(&cache_path, json.as_bytes())
        .await
        .context("Failed to write cache file")?;

    Ok(())
}
