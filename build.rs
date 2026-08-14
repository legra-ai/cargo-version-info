//! Build script that computes version using the same logic as
//! cargo-version-info.
//!
//! This sets CARGO_PKG_VERSION to the computed version based on:
//! 1. BUILD_VERSION env var (CI workflows)
//! 2. CARGO_PKG_VERSION_OVERRIDE env var (legacy)
//! 3. Cargo.toml version + git SHA
//! 4. Git SHA fallback: 0.0.0-dev-<short-sha>
//!
//! Also installs git hooks via Rhusky to enforce code quality.
//!
//! Note: GitHub API fallback is skipped in build.rs to avoid heavy
//! dependencies.

use std::path::PathBuf;
use std::{
    env,
    fs,
};

use rhusky::Rhusky;

fn main() {
    Rhusky::new()
        .hooks_dir(".githooks")
        .skip_in_env("GITHUB_ACTIONS")
        .with_default_hooks()
        .install_from_build_script()
        .expect("failed to install repository Git hooks");

    let version = compute_version_string(".").unwrap_or_else(|e| {
        eprintln!(
            "cargo:warning=Version computation failed: {}, using fallback",
            e
        );
        "0.0.0-dev-unknown".to_string()
    });

    println!("cargo:rustc-env=CARGO_PKG_VERSION={}", version);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-env-changed=BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION_OVERRIDE");
}

fn compute_version_string(
    repo_path: impl Into<PathBuf>,
) -> Result<String, Box<dyn std::error::Error>> {
    let repo_root: PathBuf = repo_path.into();
    let manifest = repo_root.join("Cargo.toml");

    // Try explicit overrides first (CI workflow should set BUILD_VERSION)
    let env_version = ["BUILD_VERSION", "CARGO_PKG_VERSION_OVERRIDE"]
        .into_iter()
        .find_map(|key| env::var(key).ok())
        .filter(|v| !v.trim().is_empty());

    if let Some(version) = env_version {
        return Ok(version);
    }

    // Fall back to manifest version (from Cargo.toml), optionally append SHA if
    // available
    if let Some(manifest_version) = read_manifest_version(&manifest) {
        let trimmed = manifest_version.trim();
        if !trimmed.is_empty() && trimmed != "0.0.0" {
            let version_with_sha = short_sha(&repo_root)
                .map(|sha| format!("{trimmed}-{sha}"))
                .unwrap_or_else(|| trimmed.to_string());
            return Ok(version_with_sha);
        }
    }

    // Final fallback: git SHA for local dev
    let repo = gix::discover(&repo_root)
        .map_err(|e| format!("Failed to discover git repository: {}", e))?;

    let head = repo
        .head()
        .map_err(|e| format!("Failed to read HEAD: {}", e))?;
    let commit_id = head
        .id()
        .ok_or_else(|| "HEAD does not point to a commit".to_string())?;
    let short_sha = commit_id
        .shorten()
        .map_err(|e| format!("Failed to shorten commit SHA: {}", e))?;

    Ok(format!("0.0.0-dev-{}", short_sha))
}

fn short_sha(repo_path: &PathBuf) -> Option<String> {
    let repo = gix::discover(repo_path).ok()?;
    let head = repo.head().ok()?;
    let commit_id = head.id()?;
    let short = commit_id.shorten().ok()?;
    Some(short.to_string())
}

fn read_manifest_version(manifest: &PathBuf) -> Option<String> {
    let contents = fs::read_to_string(manifest).ok()?;
    let value: toml::Value = toml::from_str(&contents).ok()?;
    value
        .get("package")
        .and_then(|pkg| pkg.get("version"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}
