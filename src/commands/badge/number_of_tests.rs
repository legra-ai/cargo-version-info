//! Generate number of tests badge.

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

/// Show the number of tests badge.
pub async fn badge_number_of_tests(
    writer: &mut dyn std::io::Write,
    package: &cargo_metadata::Package,
) -> Result<()> {
    let mut logger = cargo_plugin_utils::logger::Logger::new();
    // Use ephemeral status (cyan) for subprocess operations
    logger.status("Generating", "test count badge");

    let test_count = get_test_count(&mut logger, package).await?;

    if let Some(count) = test_count {
        let badge_url = format!("https://img.shields.io/badge/tests-{}-blue", count);
        let badge_markdown = format!("[![Tests]({})](tests/)", badge_url);
        writeln!(writer, "{}", badge_markdown)?;
    }

    Ok(())
}

/// Cache entry for test count results.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestCountCache {
    /// Package name
    package: String,
    /// Cache key (git commit hash or file mtime)
    cache_key: String,
    /// Test count
    test_count: u32,
}

/// Get the number of tests in the package.
/// Uses cache if available and valid.
async fn get_test_count(
    logger: &mut cargo_plugin_utils::logger::Logger,
    package: &cargo_metadata::Package,
) -> Result<Option<u32>> {
    // Try to load from cache first
    if let Some(cached) = load_test_count_cache(package).await? {
        let current_key = common::compute_cache_key(package).await?;
        if cached.cache_key == current_key && package.name == cached.package {
            return Ok(Some(cached.test_count));
        }
    }

    // Use cargo test --no-run --message-format=json to count tests
    let package_name = package.name.clone();
    let output = cargo_plugin_utils::logger::run_subprocess(
        logger,
        move || {
            let mut cmd = CommandBuilder::new("cargo");
            cmd.arg("test");
            cmd.arg("--package");
            cmd.arg(package_name.as_str());
            cmd.arg("--no-run");
            cmd.arg("--message-format");
            cmd.arg("json");
            cmd
        },
        None,
    )
    .await?;

    if !output.success() {
        return Ok(None);
    }

    // Parse JSON messages to count test artifacts
    let stdout = output
        .stdout_str()
        .context("Failed to parse cargo test output")?;

    let mut test_count = 0;
    let package_id_prefix = format!("{}@", package.name);
    for line in stdout.lines() {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        // Look for compiler artifacts that are test executables for our package
        if json.get("reason") != Some(&serde_json::Value::String("compiler-artifact".to_string())) {
            continue;
        }

        // Check if this is for our package
        let is_our_package = json
            .get("package_id")
            .and_then(|id| id.as_str())
            .map(|id| id.starts_with(&package_id_prefix))
            .unwrap_or(false);

        if !is_our_package {
            continue;
        }

        // Check if it's a test target with an executable
        let is_test = json
            .get("target")
            .and_then(|t| t.get("kind"))
            .and_then(|k| k.as_array())
            .map(|kinds| kinds.contains(&serde_json::Value::String("test".to_string())))
            .unwrap_or(false);

        if !is_test {
            continue;
        }

        // Count test executables
        if let Some(executable) = json.get("executable")
            && executable.is_string()
        {
            test_count += 1;
        }
    }

    // If we got a count from JSON parsing, use it
    if test_count > 0 {
        // Save to cache
        save_test_count_cache(package, test_count).await?;
        return Ok(Some(test_count));
    }

    // Alternative: count by running test binaries with --list flag
    // First ensure tests are compiled, then run with --list to get test names
    let package_name = package.name.clone();
    let compile_output = cargo_plugin_utils::logger::run_subprocess(
        logger,
        {
            let package_name = package_name.clone();
            move || {
                let mut cmd = CommandBuilder::new("cargo");
                cmd.arg("test");
                cmd.arg("--package");
                cmd.arg(package_name.as_str());
                cmd.arg("--no-run");
                cmd
            }
        },
        None,
    )
    .await?;

    if !compile_output.success() {
        return Ok(None);
    }

    // Then run with --list to get test names
    let list_output = cargo_plugin_utils::logger::run_subprocess(
        logger,
        move || {
            let mut cmd = CommandBuilder::new("cargo");
            cmd.arg("test");
            cmd.arg("--package");
            cmd.arg(package_name.as_str());
            cmd.arg("--");
            cmd.arg("--list");
            cmd
        },
        None,
    )
    .await?;

    if list_output.success() {
        let list_stdout = list_output
            .stdout_str()
            .context("Failed to parse cargo test --list output")?;

        // Count lines that are test names (format: "test_name: test")
        let count = list_stdout
            .lines()
            .filter(|line| line.contains(": test"))
            .count() as u32;

        if count > 0 {
            // Save to cache
            save_test_count_cache(package, count).await?;
            return Ok(Some(count));
        }
    }

    Ok(None)
}

/// Load test count from cache.
async fn load_test_count_cache(
    _package: &cargo_metadata::Package,
) -> Result<Option<TestCountCache>> {
    let cache_path = common::get_badge_cache_path("test-count").await?;

    if !async_fs_io::try_exists(&cache_path).await? {
        return Ok(None);
    }

    let contents = async_fs_io::read_string_bounded(&cache_path, 16 * 1024 * 1024)
        .await
        .context("Failed to read cache file")?;

    let cache: TestCountCache =
        serde_json::from_str(&contents).context("Failed to parse cache file")?;

    Ok(Some(cache))
}

/// Save test count to cache.
async fn save_test_count_cache(package: &cargo_metadata::Package, test_count: u32) -> Result<()> {
    let cache_key = common::compute_cache_key(package).await?;
    let cache = TestCountCache {
        package: package.name.to_string(),
        cache_key,
        test_count,
    };

    let cache_path = common::get_badge_cache_path("test-count").await?;

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
