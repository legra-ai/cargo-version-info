//! GitHub API integration for version queries.

use std::env;

use anyhow::{
    Context,
    Result,
};

use crate::version::{
    format_version,
    increment_patch,
    parse_version,
};

/// Get the latest published release version from GitHub.
///
/// Uses the GitHub API via octocrab. Works for public repos without a token
/// (with rate limits). For private repos, a token is required (automatically
/// detected from GITHUB_TOKEN env var if not provided).
#[allow(clippy::disallowed_methods)] // CLI tool needs direct env access
pub async fn get_latest_release_version(
    owner: &str,
    repo: &str,
    github_token: Option<&str>,
) -> Result<Option<String>> {
    // Auto-detect token from environment if not provided
    let env_token = env::var("GITHUB_TOKEN").ok();
    let token = github_token.or(env_token.as_deref());

    // Try with token first (required for private repos, better rate limits for
    // public)
    let result = if let Some(token) = token {
        get_latest_release_via_api(owner, repo, Some(token)).await
    } else {
        // Try without token (public repos only)
        get_latest_release_via_api(owner, repo, None).await
    };

    match result {
        Ok(version) => Ok(Some(version)),
        Err(e) => {
            let error_msg = e.to_string();
            // If no releases found, return None instead of error
            if error_msg.contains("No releases found") {
                Ok(None)
            } else if error_msg.contains("404") || error_msg.contains("Not Found") {
                // 404 could mean private repo without auth or repo doesn't exist
                if token.is_none() {
                    Err(anyhow::anyhow!(
                        "Repository not found or is private. For private repositories, \
                         set GITHUB_TOKEN environment variable or pass --github-token"
                    )
                    .context(error_msg))
                } else {
                    Err(e)
                }
            } else if error_msg.contains("403") || error_msg.contains("Forbidden") {
                // 403 usually means private repo or rate limit
                Err(anyhow::anyhow!(
                    "Access forbidden. This may be a private repository. \
                     Ensure GITHUB_TOKEN has appropriate permissions."
                )
                .context(error_msg))
            } else {
                Err(e)
            }
        }
    }
}

/// Get latest release via GitHub API.
///
/// Works for public repositories even without a token (with rate limits).
/// If a token is provided, uses it for authentication (higher rate limits).
async fn get_latest_release_via_api(
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<String> {
    let octocrab = if let Some(token) = token {
        octocrab::OctocrabBuilder::new()
            .personal_token(token.to_string())
            .build()
            .context("Failed to create GitHub API client")?
    } else {
        // For public repos, we can use octocrab without a token
        octocrab::Octocrab::builder()
            .build()
            .context("Failed to create GitHub API client")?
    };

    let releases = octocrab
        .repos(owner, repo)
        .releases()
        .list()
        .per_page(1)
        .send()
        .await
        .context("Failed to query GitHub releases")?;

    let release = releases.items.first().context("No releases found")?;

    let tag_name = release.tag_name.as_str();
    let version = tag_name.strip_prefix('v').unwrap_or(tag_name);
    let version = version.strip_prefix('V').unwrap_or(version);

    Ok(version.to_string())
}

/// Get the latest version from git tags.
///
/// Queries git tags in the current repository to find the latest semantic
/// version tag. Returns None if no version tags exist.
fn get_latest_git_tag_version() -> Result<Option<String>> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = gix::discover(cwd)
        .context("Failed to discover git repository. Ensure you're in a git repository.")?;

    let mut version_tags: Vec<(String, (u32, u32, u32))> = repo
        .references()?
        .prefixed("refs/tags/")?
        .filter_map(|r: Result<gix::Reference<'_>, _>| r.ok())
        .filter_map(|r| {
            let name_full = r.name().as_bstr().to_string();
            let name = name_full.strip_prefix("refs/tags/").unwrap_or(&name_full);
            let version_str = name
                .strip_prefix('v')
                .or_else(|| name.strip_prefix('V'))
                .unwrap_or(name);

            // Try to parse as semantic version
            if let Ok((major, minor, patch)) = parse_version(version_str) {
                Some((name.to_string(), (major, minor, patch)))
            } else {
                None
            }
        })
        .collect();

    // Sort tags by semantic version (major, minor, patch)
    version_tags.sort_by_key(|a| a.1);

    Ok(version_tags
        .last()
        .map(|(tag_name, _): &(String, (u32, u32, u32))| {
            tag_name
                .strip_prefix('v')
                .or_else(|| tag_name.strip_prefix('V'))
                .unwrap_or(tag_name)
                .to_string()
        }))
}

/// Calculate next patch version from latest git tag.
///
/// Queries git tags in the current repository (not GitHub releases) to find
/// the latest version. If no tags exist, returns "0.0.0" as latest and
/// "0.0.1" as next.
pub async fn calculate_next_version(
    _owner: &str,
    _repo: &str,
    _github_token: Option<&str>,
) -> Result<(String, String)> {
    // Get latest version from git tags (not GitHub releases)
    let latest_version_str = match get_latest_git_tag_version()? {
        Some(v) => v,
        None => {
            // No tags yet, start at 0.0.1
            return Ok(("0.0.0".to_string(), "0.0.1".to_string()));
        }
    };

    let (major, minor, patch) = parse_version(&latest_version_str)
        .with_context(|| format!("Failed to parse latest version: {}", latest_version_str))?;

    let (major, minor, patch) = increment_patch(major, minor, patch);
    let next_version = format_version(major, minor, patch);

    Ok((latest_version_str, next_version))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    async fn create_test_git_repo_with_tags(tags: &[&str]) -> async_fs_io::TempDir {
        let dir = async_fs_io::TempDir::create(std::env::temp_dir())
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

        // Create an initial commit
        async_fs_io::write_bytes(dir.path().join("README.md"), b"# Test\n")
            .await
            .unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Create tags
        for tag in tags {
            Command::new("git")
                .args(["tag", "-a", tag, "-m", &format!("Release {}", tag)])
                .current_dir(dir.path())
                .output()
                .unwrap();
        }

        dir
    }

    #[tokio::test]
    async fn test_get_latest_git_tag_version_no_tags() {
        let dir = create_test_git_repo_with_tags(&[]).await;
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(dir.path()).unwrap();
        let result = get_latest_git_tag_version().unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_get_latest_git_tag_version_single_tag() {
        let _dir = create_test_git_repo_with_tags(&["v0.1.0"]).await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();
        let result = get_latest_git_tag_version().unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(result, Some("0.1.0".to_string()));
    }

    #[tokio::test]
    async fn test_get_latest_git_tag_version_multiple_tags() {
        let _dir = create_test_git_repo_with_tags(&["v0.1.0", "v0.2.0", "v0.1.5"]).await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();
        let result = get_latest_git_tag_version().unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        // Should return the latest version (0.2.0)
        assert_eq!(result, Some("0.2.0".to_string()));
    }

    #[tokio::test]
    async fn test_get_latest_git_tag_version_without_v_prefix() {
        let _dir = create_test_git_repo_with_tags(&["0.3.0", "v0.2.0"]).await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();
        let result = get_latest_git_tag_version().unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        // Should return the latest version (0.3.0)
        assert_eq!(result, Some("0.3.0".to_string()));
    }

    #[tokio::test]
    async fn test_calculate_next_version_no_tags() {
        let _dir = create_test_git_repo_with_tags(&[]).await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();
        let (latest, next) = calculate_next_version("test", "repo", None).await.unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(latest, "0.0.0");
        assert_eq!(next, "0.0.1");
    }

    #[tokio::test]
    async fn test_calculate_next_version_with_tags() {
        let _dir = create_test_git_repo_with_tags(&["v0.1.2"]).await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();
        let (latest, next) = calculate_next_version("test", "repo", None).await.unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(latest, "0.1.2");
        assert_eq!(next, "0.1.3");
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_get_latest_release_via_api() {
        // This test requires network access
        // Only run manually
        if let Ok(Some(version)) = get_latest_release_version("rust-lang", "rust", None).await {
            println!("Latest rust release: {}", version);
        }
    }
}
