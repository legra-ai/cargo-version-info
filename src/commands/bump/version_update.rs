//! TOML version update logic.
//!
//! This module handles updating the version field in Cargo.toml files while
//! preserving all formatting, comments, and structure. It uses the `toml_edit`
//! crate (same as `cargo-edit`) to ensure maximum compatibility with cargo's
//! own TOML handling.
//!
//! # Design Philosophy
//!
//! - **Preserve Everything**: Comments, whitespace, formatting are all
//!   preserved
//! - **Workspace Support**: Handles both `[package]` and `[workspace.package]`
//! - **Minimal Changes**: Only modifies the `version` field, nothing else
//!
//! # Examples
//!
//! ```rust,no_run
//! use std::path::Path;
//!
//! # use anyhow::Result;
//! # async fn example() -> Result<()> {
//! use cargo_version_info::commands::bump::version_update::update_cargo_toml_version;
//!
//! let manifest = Path::new("Cargo.toml");
//! update_cargo_toml_version(manifest, "0.1.0", "0.2.0").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Implementation Details
//!
//! The function uses `toml_edit::DocumentMut` which provides a mutable view
//! of a TOML document while tracking formatting information. This allows us
//! to modify specific values without affecting the rest of the file.
//!
//! ## Version Location
//!
//! The version can be in one of two locations:
//!
//! 1. **Package section**: `[package] version = "X.Y.Z"`
//! 2. **Workspace section**: `[workspace.package] version = "X.Y.Z"`
//!
//! We check both locations and update whichever is found.

use std::path::Path;

use anyhow::{
    Context,
    Result,
};
use async_fs_io::{
    read_string_bounded,
    write_bytes,
};
use toml_edit::{
    DocumentMut,
    value,
};

/// Update the version field in a Cargo.toml file.
///
/// This function parses the TOML file, locates the version field (in either
/// `[package]` or `[workspace.package]`), updates it, and writes the file back
/// while preserving all formatting.
///
/// # Arguments
///
/// * `manifest_path` - Path to the Cargo.toml file
/// * `_old_version` - The current version (unused, kept for API consistency)
/// * `new_version` - The target version to set
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be read
/// - The TOML is invalid
/// - No `[package]` or `[workspace.package]` section is found
/// - The file cannot be written
///
/// # Examples
///
/// ```rust,no_run
/// # use std::path::Path;
/// # use anyhow::Result;
/// # async fn example() -> Result<()> {
/// use cargo_version_info::commands::bump::version_update::update_cargo_toml_version;
///
/// let manifest = Path::new("./Cargo.toml");
/// update_cargo_toml_version(manifest, "1.0.0", "1.1.0").await?;
/// # Ok(())
/// # }
/// ```
///
/// # Formatting Preservation
///
/// This function uses `toml_edit` to ensure that:
/// - Comments are preserved
/// - Whitespace and indentation are maintained
/// - Table order is unchanged
/// - Only the version value is modified
///
/// Before:
/// ```toml
/// [package]
/// name = "my-crate"  # Important crate
/// version = "0.1.0"  # Current version
/// edition = "2021"
/// ```
///
/// After calling `update_cargo_toml_version(path, "0.1.0", "0.2.0")`:
/// ```toml
/// [package]
/// name = "my-crate"  # Important crate
/// version = "0.2.0"  # Current version
/// edition = "2021"
/// ```
pub async fn update_cargo_toml_version(
    manifest_path: &Path,
    _old_version: &str,
    new_version: &str,
) -> Result<()> {
    // Read the current content
    let content = read_string_bounded(manifest_path, 16 * 1024 * 1024)
        .await
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;

    // Parse the TOML document while preserving formatting
    // This creates a DocumentMut which tracks all formatting information
    let mut doc = content
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to parse TOML in {}", manifest_path.display()))?;

    // Track whether we updated each section
    let mut package_updated = false;
    let mut workspace_updated = false;

    // Update [package] version if present AND has an explicit version field
    // (not version.workspace = true)
    if let Some(package) = doc.get_mut("package").and_then(|p| p.as_table_mut())
        && package.contains_key("version")
        // Check if version is a string (explicit) vs table (workspace inheritance)
        && package.get("version").is_some_and(|v| v.as_str().is_some())
    {
        package.insert("version", value(new_version));
        package_updated = true;
    }

    // Update [workspace.package] version if present
    if let Some(workspace_package) = doc
        .get_mut("workspace")
        .and_then(|w| w.as_table_mut())
        .and_then(|w| w.get_mut("package"))
        .and_then(|p| p.as_table_mut())
        && workspace_package.contains_key("version")
    {
        workspace_package.insert("version", value(new_version));
        workspace_updated = true;
    }

    if !package_updated && !workspace_updated {
        anyhow::bail!(
            "Could not find version in [package] or [workspace.package] section in {}",
            manifest_path.display()
        );
    }

    // Write back the modified document
    // The to_string() method serializes the document while preserving all
    // formatting that was tracked during parsing
    let serialized = doc.to_string();
    write_bytes(manifest_path, serialized.as_bytes())
        .await
        .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_temp_manifest(content: &str) -> (async_fs_io::TempDir, std::path::PathBuf) {
        let dir = async_fs_io::TempDir::create(std::env::temp_dir())
            .await
            .unwrap();
        let manifest_path = dir.path().join("Cargo.toml");
        async_fs_io::write_bytes(&manifest_path, content.as_bytes())
            .await
            .unwrap();
        (dir, manifest_path)
    }

    #[tokio::test]
    async fn test_update_package_version() {
        let (_dir, manifest_path) = create_temp_manifest(
            r#"[package]
name = "test"
version = "0.1.0"
"#,
        )
        .await;

        update_cargo_toml_version(&manifest_path, "0.1.0", "0.2.0")
            .await
            .unwrap();

        let content = async_fs_io::read_string_bounded(&manifest_path, 64 * 1024 * 1024)
            .await
            .unwrap();
        assert!(content.contains("version = \"0.2.0\""));
        assert!(!content.contains("0.1.0"));
    }

    #[tokio::test]
    async fn test_update_workspace_package_version() {
        let (_dir, manifest_path) = create_temp_manifest(
            r#"[workspace.package]
version = "1.0.0"
"#,
        )
        .await;

        update_cargo_toml_version(&manifest_path, "1.0.0", "2.0.0")
            .await
            .unwrap();

        let content = async_fs_io::read_string_bounded(&manifest_path, 64 * 1024 * 1024)
            .await
            .unwrap();
        assert!(content.contains("version = \"2.0.0\""));
    }

    #[tokio::test]
    async fn test_preserves_formatting() {
        let (_dir, manifest_path) = create_temp_manifest(
            r#"[package]
name = "test"  # Package name
version = "0.1.0"
edition = "2021"
"#,
        )
        .await;

        update_cargo_toml_version(&manifest_path, "0.1.0", "0.2.0")
            .await
            .unwrap();

        let content = async_fs_io::read_string_bounded(&manifest_path, 64 * 1024 * 1024)
            .await
            .unwrap();
        // Verify comments are preserved
        assert!(content.contains("# Package name"));
        // Verify version was updated
        assert!(content.contains("version = \"0.2.0\""));
        // Verify version comment still exists (though toml_edit may reformat it)
        assert!(!content.contains("0.1.0"));
    }

    #[tokio::test]
    async fn test_update_both_package_and_workspace_version() {
        // Test case: Cargo.toml with both [workspace.package] and [package]
        // having explicit version fields (like dotenvage)
        let (_dir, manifest_path) = create_temp_manifest(
            r#"[workspace]
members = ["npm/dotenvage-napi"]

[workspace.package]
version = "0.2.1"
edition = "2024"

[package]
name = "dotenvage"
version = "0.2.1"
edition.workspace = true
"#,
        )
        .await;

        update_cargo_toml_version(&manifest_path, "0.2.1", "0.2.2")
            .await
            .unwrap();

        let content = async_fs_io::read_string_bounded(&manifest_path, 64 * 1024 * 1024)
            .await
            .unwrap();
        // Both sections should be updated
        assert!(!content.contains("0.2.1"), "Old version should be gone");
        // Count occurrences of new version - should appear twice
        let count = content.matches("\"0.2.2\"").count();
        assert_eq!(count, 2, "New version should appear in both sections");
    }

    #[tokio::test]
    async fn test_package_with_workspace_inheritance_not_updated() {
        // Test case: [package] version inherits from workspace (version.workspace =
        // true) Only [workspace.package] should be updated
        let (_dir, manifest_path) = create_temp_manifest(
            r#"[workspace.package]
version = "1.0.0"

[package]
name = "test"
version.workspace = true
"#,
        )
        .await;

        update_cargo_toml_version(&manifest_path, "1.0.0", "2.0.0")
            .await
            .unwrap();

        let content = async_fs_io::read_string_bounded(&manifest_path, 64 * 1024 * 1024)
            .await
            .unwrap();
        // workspace.package should be updated
        assert!(content.contains("[workspace.package]\nversion = \"2.0.0\""));
        // package should still have workspace inheritance
        assert!(content.contains("version.workspace = true"));
    }

    #[tokio::test]
    async fn test_no_package_section_error() {
        let (_dir, manifest_path) = create_temp_manifest(
            r#"[dependencies]
some-crate = "1.0"
"#,
        )
        .await;

        let result = update_cargo_toml_version(&manifest_path, "0.1.0", "0.2.0").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Could not find"));
    }
}
