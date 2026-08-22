//! README.md version reference updates.
//!
//! This module detects and updates version references in README.md files.
//! When bumping a version, dependency installation instructions often show
//! outdated versions, for example:
//!
//! ```toml
//! [dependencies]
//! my-crate = "0.0.1"  # Should be updated to new version
//! ```
//!
//! # Detected Patterns
//!
//! The module searches for patterns like:
//! - `crate-name = "X.Y.Z"` (TOML dependency format)
//!
//! Both hyphenated (`my-crate`) and underscored (`my_crate`) names are handled.

use std::path::Path;

use anyhow::{
    Context,
    Result,
};
use async_fs_io::read_string_bounded;
use regex::Regex;

/// Result of updating README version references.
#[derive(Debug)]
pub struct ReadmeUpdateResult {
    /// The updated content (or original if no changes).
    pub content: String,
    /// Whether any changes were made.
    pub modified: bool,
}

/// Update version references in README.md content.
///
/// Searches for patterns like `package-name = "old_version"` and replaces
/// them with the new version.
///
/// # Arguments
///
/// * `content` - The README.md content to update
/// * `package_name` - The crate/package name to look for
/// * `old_version` - The version to replace
/// * `new_version` - The new version to use
///
/// # Returns
///
/// Returns `ReadmeUpdateResult` with the (potentially modified) content
/// and a flag indicating if changes were made.
///
/// # Examples
///
/// ```
/// use cargo_version_info::commands::bump::readme_update::update_readme_content;
///
/// let content = r#"my-crate = "0.1.0""#;
///
/// let result = update_readme_content(content, "my-crate", "0.1.0", "0.2.0");
/// assert!(result.modified);
/// assert!(result.content.contains(r#"my-crate = "0.2.0""#));
/// ```
pub fn update_readme_content(
    content: &str,
    package_name: &str,
    old_version: &str,
    new_version: &str,
) -> ReadmeUpdateResult {
    // Handle both hyphenated and underscored package names
    // Cargo treats them as equivalent
    let hyphenated = package_name.replace('_', "-");
    let underscored = package_name.replace('-', "_");

    let mut result = content.to_string();
    let mut modified = false;

    // Try both naming conventions
    for name in [&hyphenated, &underscored] {
        // Pattern: name = "version" (with optional whitespace)
        // Captures: (name = ")version(")
        let pattern = format!(
            r#"({})\s*=\s*"{}""#,
            regex::escape(name),
            regex::escape(old_version)
        );

        if let Ok(re) = Regex::new(&pattern) {
            let replacement = format!(r#"{} = "{}""#, name, new_version);
            let new_result = re.replace_all(&result, replacement.as_str());
            if new_result != result {
                result = new_result.into_owned();
                modified = true;
            }
        }
    }

    ReadmeUpdateResult {
        content: result,
        modified,
    }
}

/// Update version references in a README.md file.
///
/// Reads the file, updates version references, and returns the result.
/// Does NOT write the file - that's handled by the caller.
///
/// # Arguments
///
/// * `readme_path` - Path to the README.md file
/// * `package_name` - The crate/package name to look for
/// * `old_version` - The version to replace
/// * `new_version` - The new version to use
///
/// # Returns
///
/// Returns `Ok(Some(ReadmeUpdateResult))` if the file exists and was processed,
/// `Ok(None)` if the file doesn't exist, or an error if reading failed.
pub async fn update_readme_file(
    readme_path: &Path,
    package_name: &str,
    old_version: &str,
    new_version: &str,
) -> Result<Option<ReadmeUpdateResult>> {
    if !async_fs_io::try_exists(readme_path).await? {
        return Ok(None);
    }

    let content = read_string_bounded(readme_path, 16 * 1024 * 1024)
        .await
        .with_context(|| format!("Failed to read {}", readme_path.display()))?;

    let result = update_readme_content(&content, package_name, old_version, new_version);
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_update_simple_version() {
        let content = r#"
Add to your Cargo.toml:

```toml
[dependencies]
my-crate = "0.1.0"
```
"#;

        let result = update_readme_content(content, "my-crate", "0.1.0", "0.2.0");
        assert!(result.modified);
        assert!(result.content.contains(r#"my-crate = "0.2.0""#));
        assert!(!result.content.contains(r#"my-crate = "0.1.0""#));
    }

    #[tokio::test]
    async fn test_update_underscored_name() {
        let content = r#"my_crate = "1.0.0""#;

        let result = update_readme_content(content, "my-crate", "1.0.0", "1.1.0");
        assert!(result.modified);
        assert!(result.content.contains(r#"my_crate = "1.1.0""#));
    }

    #[tokio::test]
    async fn test_no_match_different_version() {
        let content = r#"my-crate = "0.1.0""#;

        let result = update_readme_content(content, "my-crate", "0.2.0", "0.3.0");
        assert!(!result.modified);
        assert!(result.content.contains(r#"my-crate = "0.1.0""#));
    }

    #[tokio::test]
    async fn test_no_match_different_crate() {
        let content = r#"other-crate = "0.1.0""#;

        let result = update_readme_content(content, "my-crate", "0.1.0", "0.2.0");
        assert!(!result.modified);
    }

    #[tokio::test]
    async fn test_multiple_occurrences() {
        let content = r#"
my-crate = "0.1.0"
# Also:
my-crate = "0.1.0"
"#;

        let result = update_readme_content(content, "my-crate", "0.1.0", "0.2.0");
        assert!(result.modified);
        // Both should be updated
        assert_eq!(result.content.matches(r#"my-crate = "0.2.0""#).count(), 2);
    }

    #[tokio::test]
    async fn test_whitespace_variations() {
        let content = r#"my-crate="0.1.0""#;

        let result = update_readme_content(content, "my-crate", "0.1.0", "0.2.0");
        assert!(result.modified);
        assert!(result.content.contains(r#"my-crate = "0.2.0""#));
    }
}
