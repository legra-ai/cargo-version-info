//! Diff generation and hunk-level filtering.
//!
//! This module implements the "holy grail" of version bumping: staging ONLY
//! the lines that contain version changes, leaving all other changes (even
//! in the same file) unstaged.
//!
//! # The Problem
//!
//! Consider this scenario - you're working on Cargo.toml:
//!
//! ```diff
//! @@ -1,7 +1,8 @@
//!  [package]
//!  name = "my-crate"
//! -version = "0.1.0"
//! +version = "0.2.0"
//!  edition = "2021"
//!  
//!  [dependencies]
//! -serde = "1.0"
//! +serde = { version = "1.0", features = ["derive"] }
//! ```
//!
//! We want to commit ONLY the version line change, not the serde dependency
//! change. This requires hunk-level staging.
//!
//! # Solution: Unified Diff + Hunk Filtering
//!
//! 1. Generate unified diff between HEAD and working directory
//! 2. Parse diff into hunks
//! 3. Filter hunks to find version-related changes
//! 4. Apply only those hunks to create a partially-staged file
//! 5. Write the partially-staged content as a blob
//!
//! # Unified Diff Format
//!
//! A unified diff looks like:
//! ```text
//! --- a/Cargo.toml
//! +++ b/Cargo.toml
//! @@ -3,5 +3,5 @@
//!  name = "my-crate"
//! -version = "0.1.0"
//! +version = "0.2.0"
//!  edition = "2021"
//! ```
//!
//! Key components:
//! - **Hunk header**: `@@ -3,5 +3,5 @@` means "at line 3, remove 5 lines, add 5
//!   lines"
//! - **Context lines**: Start with space, unchanged
//! - **Removed lines**: Start with `-`
//! - **Added lines**: Start with `+`
//!
//! # Hunk Filtering Logic
//!
//! A hunk is "version-related" if:
//! - It contains lines with "version" keyword
//! - It contains the old or new version string
//! - It's within a reasonable distance of other version changes
//!
//! # Implementation Strategy
//!
//! We use the `similar` crate to:
//! - Generate line-by-line diff
//! - Identify change regions (hunks)
//! - Reconstruct file content with selected changes only

use std::collections::HashSet;

use anyhow::{
    Context,
    Result,
};
use regex::Regex;
use similar::{
    ChangeTag,
    TextDiff,
};

/// Apply only version-related hunks to create partially-staged content.
///
/// This is the core function that implements selective hunk staging. It:
/// 1. Generates a diff between HEAD and working directory versions
/// 2. Identifies which lines changed
/// 3. Filters to keep only version-related changes
/// 4. Reconstructs the file with only those changes applied
///
/// # Arguments
///
/// * `head_content` - Content of the file in HEAD commit
/// * `working_content` - Content of the file in working directory
/// * `old_version` - The version string being replaced
/// * `new_version` - The version string being added
///
/// # Returns
///
/// Returns the partially-staged content (HEAD + only version changes).
///
/// # Examples
///
/// ```rust
/// # use cargo_version_info::commands::bump::diff::apply_version_hunks;
/// let head = "[package]\nname = \"test\"\nversion = \"0.1.0\"\ndesc = \"old\"";
/// let working = "[package]\nname = \"test\"\nversion = \"0.2.0\"\ndesc = \"new\"";
///
/// let staged = apply_version_hunks(head, working, "0.1.0", "0.2.0").unwrap();
///
/// // staged contains only the version change, not the desc change
/// assert!(staged.contains("version = \"0.2.0\""));
/// assert!(staged.contains("desc = \"old\"")); // NOT "new"
/// ```
///
/// # Algorithm
///
/// 1. Generate unified diff using `similar::TextDiff`
/// 2. Iterate through all changes (insertions, deletions, unchanged)
/// 3. For each change, check if it's version-related:
///    - Does the line contain "version"?
///    - Does the line contain old_version or new_version?
/// 4. Build output:
///    - Version-related changes: Use working directory version
///    - Non-version changes: Use HEAD version (ignore working changes)
///    - Unchanged lines: Include as-is
///
/// # Edge Cases
///
/// - **Multiple version fields**: All are updated (package.version,
///   dependencies.*.version)
/// - **Version in comments**: May be incorrectly detected (acceptable
///   trade-off)
/// - **Adjacent changes**: Non-version changes adjacent to version changes are
///   kept separate
pub fn apply_version_hunks(
    head_content: &str,
    working_content: &str,
    old_version: &str,
    new_version: &str,
) -> Result<String> {
    // Generate unified diff between HEAD and working directory
    let diff = TextDiff::from_lines(head_content, working_content);

    let mut result = Vec::new();

    // Iterate through all changes
    for change in diff.iter_all_changes() {
        let line = change.value();

        // Determine if this line is version-related
        let is_version_related =
            line.contains("version") || line.contains(old_version) || line.contains(new_version);

        match change.tag() {
            ChangeTag::Equal => {
                // Unchanged line - always include
                result.push(line);
            }
            ChangeTag::Delete => {
                // Line removed in working directory
                if is_version_related {
                    // This is a version line being removed - apply the change
                    // (skip it) Don't add to result
                } else {
                    // Non-version line removed - keep the original (don't apply change)
                    result.push(line);
                }
            }
            ChangeTag::Insert => {
                // Line added in working directory
                if is_version_related {
                    // This is a version line being added - apply the change (include it)
                    result.push(line);
                } else {
                    // Non-version line added - don't apply the change (skip it)
                    // The line stays not present (remains as in HEAD)
                }
            }
        }
    }

    Ok(result.join(""))
}

/// Check if the file has changes beyond version modifications.
///
/// This is used to determine if we need hunk-level filtering or if we can
/// just stage the whole file.
///
/// # Arguments
///
/// * `head_content` - Content from HEAD
/// * `working_content` - Content from working directory
/// * `old_version` - Old version string
/// * `new_version` - New version string
///
/// # Returns
///
/// Returns `true` if there are non-version changes.
pub fn has_non_version_changes(
    head_content: &str,
    working_content: &str,
    old_version: &str,
    new_version: &str,
) -> bool {
    let diff = TextDiff::from_lines(head_content, working_content);

    // Check if any changes are NOT version-related
    for change in diff.iter_all_changes() {
        if matches!(change.tag(), ChangeTag::Delete | ChangeTag::Insert) {
            let line = change.value();
            let is_version_related = line.contains("version")
                || line.contains(old_version)
                || line.contains(new_version);

            if !is_version_related {
                // Found a non-version change
                return true;
            }
        }
    }

    false
}

/// Check if a line is version-related for README.md.
///
/// A line is README-version-related if it matches the pattern
/// `crate-name = "version"` (typical TOML dependency format).
///
/// # Arguments
///
/// * `line` - The line to check
/// * `crate_name` - The crate name (hyphenated or underscored)
/// * `old_version` - The version being replaced
/// * `new_version` - The version being added
fn is_readme_version_line(
    line: &str,
    crate_name: &str,
    old_version: &str,
    new_version: &str,
) -> bool {
    // Handle both hyphenated and underscored package names
    let hyphenated = crate_name.replace('_', "-");
    let underscored = crate_name.replace('-', "_");

    for name in [&hyphenated, &underscored] {
        // Pattern: name = "version" (with optional whitespace)
        for version in [old_version, new_version] {
            let pattern = format!(
                r#"{}(\s|[-_])?\s*=\s*"{}""#,
                regex::escape(name),
                regex::escape(version)
            );
            if let Ok(re) = Regex::new(&pattern)
                && re.is_match(line)
            {
                return true;
            }
        }
    }

    false
}

/// Apply only README version-related hunks to create partially-staged content.
///
/// This function filters changes to README.md to include only lines that
/// reference the crate's version (e.g., `my-crate = "1.0.0"`).
///
/// # Arguments
///
/// * `head_content` - Content of README.md in HEAD commit
/// * `working_content` - Content of README.md in working directory
/// * `crate_name` - The crate/package name
/// * `old_version` - The version string being replaced
/// * `new_version` - The version string being added
///
/// # Returns
///
/// Returns the partially-staged content (HEAD + only version-related changes).
pub fn apply_readme_version_hunks(
    head_content: &str,
    working_content: &str,
    crate_name: &str,
    old_version: &str,
    new_version: &str,
) -> Result<String> {
    let diff = TextDiff::from_lines(head_content, working_content);

    let mut result = Vec::new();

    for change in diff.iter_all_changes() {
        let line = change.value();
        let is_version_related = is_readme_version_line(line, crate_name, old_version, new_version);

        match change.tag() {
            ChangeTag::Equal => {
                result.push(line);
            }
            ChangeTag::Delete => {
                if is_version_related {
                    // Version line being removed - apply the change (skip it)
                } else {
                    // Non-version line - keep original
                    result.push(line);
                }
            }
            ChangeTag::Insert => {
                if is_version_related {
                    // Version line being added - apply the change (include it)
                    result.push(line);
                } else {
                    // Non-version line - don't apply the change (skip it)
                }
            }
        }
    }

    Ok(result.join(""))
}

/// Check if README.md has changes beyond version modifications.
///
/// # Arguments
///
/// * `head_content` - Content from HEAD
/// * `working_content` - Content from working directory
/// * `crate_name` - The crate/package name
/// * `old_version` - Old version string
/// * `new_version` - New version string
///
/// # Returns
///
/// Returns `true` if there are non-version changes.
pub fn has_non_readme_version_changes(
    head_content: &str,
    working_content: &str,
    crate_name: &str,
    old_version: &str,
    new_version: &str,
) -> bool {
    let diff = TextDiff::from_lines(head_content, working_content);

    for change in diff.iter_all_changes() {
        if matches!(change.tag(), ChangeTag::Delete | ChangeTag::Insert) {
            let line = change.value();
            let is_version_related =
                is_readme_version_line(line, crate_name, old_version, new_version);

            if !is_version_related {
                return true;
            }
        }
    }

    false
}

/// Locate our crate's `[[package]]` block in a Cargo.lock by structure.
///
/// Returns the byte range `[start, end)` covering the block from its
/// `[[package]]` header up to (but not including) the next top-level
/// section header (`[[` or `[`) or end-of-file. The trailing blank line
/// between blocks is included in the range so that splicing two blocks
/// preserves Cargo.lock formatting.
///
/// Returns `None` if no `[[package]]` block names our crate.
fn find_package_block(content: &str, crate_name: &str) -> Option<(usize, usize)> {
    let target_name = format!(r#"name = "{crate_name}""#);
    let mut cursor = 0usize;
    let mut current_block_start: Option<usize> = None;

    for line in content.split_inclusive('\n') {
        let line_start = cursor;
        cursor += line.len();
        let trimmed = line.trim_end();

        if trimmed == "[[package]]" {
            if let Some(block_start) = current_block_start
                && is_local_package_block(&content[block_start..line_start], &target_name)
            {
                return Some((block_start, line_start));
            }
            current_block_start = Some(line_start);
        } else if trimmed.starts_with('[') && trimmed != "[[package]]" {
            if let Some(block_start) = current_block_start
                && is_local_package_block(&content[block_start..line_start], &target_name)
            {
                return Some((block_start, line_start));
            }
            current_block_start = None;
        }
    }

    current_block_start.and_then(|block_start| {
        is_local_package_block(&content[block_start..cursor], &target_name)
            .then_some((block_start, cursor))
    })
}

fn is_local_package_block(block: &str, target_name: &str) -> bool {
    let mut has_target_name = false;
    let mut has_source = false;

    for line in block.lines() {
        let trimmed = line.trim();
        has_target_name |= trimmed == target_name;
        has_source |= trimmed.starts_with("source = ");
    }

    has_target_name && !has_source
}

/// Splice our crate's `[[package]]` block from `working_content` into
/// `head_content`, leaving every other byte of `head_content` untouched.
///
/// This is the structural equivalent of "stage only our crate's lockfile
/// entry": dependency changes elsewhere stay unstaged. It is robust to
/// HEAD's recorded version drifting away from `old_version` (e.g. when
/// Cargo.lock was previously committed at a stale version), which the
/// older line-pattern matcher silently mishandled.
///
/// `_old_version` and `_new_version` are accepted for API compatibility
/// but no longer consulted.
pub fn apply_cargo_lock_version_hunks(
    head_content: &str,
    working_content: &str,
    crate_name: &str,
    _old_version: &str,
    _new_version: &str,
) -> Result<String> {
    let (work_start, work_end) = find_package_block(working_content, crate_name)
        .with_context(|| format!("crate `{crate_name}` not found in working Cargo.lock"))?;
    let working_block = &working_content[work_start..work_end];

    match find_package_block(head_content, crate_name) {
        Some((head_start, head_end)) => {
            let mut out = String::with_capacity(
                head_content.len() + working_block.len() - (head_end - head_start),
            );
            out.push_str(&head_content[..head_start]);
            out.push_str(working_block);
            out.push_str(&head_content[head_end..]);
            Ok(out)
        }
        // HEAD has no entry for our crate (e.g. brand-new lockfile path):
        // there is no anchor to splice into, so the cleanest choice is to
        // leave HEAD as-is. The caller will detect this via
        // has_non_cargo_lock_version_changes returning false.
        None => Ok(head_content.to_string()),
    }
}

/// Splice every local workspace member block from the working lockfile into
/// the lockfile recorded in `HEAD`.
///
/// Registry and Git dependency blocks remain byte-for-byte identical to
/// `HEAD`, even when `cargo update --workspace` refreshed them while resolving
/// the bumped workspace.
pub fn apply_workspace_cargo_lock_version_hunks(
    head_content: &str,
    working_content: &str,
    workspace_package_names: &HashSet<String>,
    old_version: &str,
    new_version: &str,
) -> Result<String> {
    let mut staged = head_content.to_string();

    for package_name in workspace_package_names {
        if find_package_block(&staged, package_name).is_none() {
            continue;
        }
        staged = apply_cargo_lock_version_hunks(
            &staged,
            working_content,
            package_name,
            old_version,
            new_version,
        )?;
    }

    Ok(staged)
}

/// Check whether a lockfile differs outside local workspace package blocks.
pub fn has_non_workspace_cargo_lock_version_changes(
    head_content: &str,
    working_content: &str,
    workspace_package_names: &HashSet<String>,
    old_version: &str,
    new_version: &str,
) -> bool {
    match apply_workspace_cargo_lock_version_hunks(
        head_content,
        working_content,
        workspace_package_names,
        old_version,
        new_version,
    ) {
        Ok(staged) => staged != working_content,
        Err(_) => true,
    }
}

/// Check if `working_content` differs from `head_content` outside our
/// crate's `[[package]]` block.
///
/// Implemented as: splice our block from working into head; if the
/// result still differs from working, the difference must lie outside
/// our block. Robust to HEAD's recorded version being arbitrarily stale.
pub fn has_non_cargo_lock_version_changes(
    head_content: &str,
    working_content: &str,
    crate_name: &str,
    _old_version: &str,
    _new_version: &str,
) -> bool {
    match apply_cargo_lock_version_hunks(head_content, working_content, crate_name, "", "") {
        Ok(staged) => staged != working_content,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_version_hunks_only_version_change() {
        let head = "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
        let working = "[package]\nname = \"test\"\nversion = \"0.2.0\"\nedition = \"2021\"\n";

        let staged = apply_version_hunks(head, working, "0.1.0", "0.2.0").unwrap();

        assert!(staged.contains("version = \"0.2.0\""));
        assert!(!staged.contains("0.1.0"));
    }

    #[test]
    fn test_apply_version_hunks_mixed_changes() {
        let head = "[package]\nname = \"test\"\nversion = \"0.1.0\"\ndescription = \"old desc\"\n";
        let working =
            "[package]\nname = \"test\"\nversion = \"0.2.0\"\ndescription = \"new desc\"\n";

        let staged = apply_version_hunks(head, working, "0.1.0", "0.2.0").unwrap();

        // Should have version change
        assert!(staged.contains("version = \"0.2.0\""));
        // Should NOT have description change - keeps old value
        assert!(staged.contains("description = \"old desc\""));
        assert!(!staged.contains("description = \"new desc\""));
    }

    #[test]
    fn test_has_non_version_changes_true() {
        let head = "[package]\nname = \"test\"\nversion = \"0.1.0\"\n";
        let working = "[package]\nname = \"test-renamed\"\nversion = \"0.2.0\"\n";

        assert!(has_non_version_changes(head, working, "0.1.0", "0.2.0"));
    }

    #[test]
    fn test_has_non_version_changes_false() {
        let head = "[package]\nname = \"test\"\nversion = \"0.1.0\"\n";
        let working = "[package]\nname = \"test\"\nversion = \"0.2.0\"\n";

        assert!(!has_non_version_changes(head, working, "0.1.0", "0.2.0"));
    }

    #[test]
    fn test_apply_version_hunks_multiple_version_fields() {
        let head =
            "[package]\nversion = \"1.0.0\"\n[dependencies]\ncrate-a = { version = \"1.0.0\" }\n";
        let working =
            "[package]\nversion = \"2.0.0\"\n[dependencies]\ncrate-a = { version = \"2.0.0\" }\n";

        let staged = apply_version_hunks(head, working, "1.0.0", "2.0.0").unwrap();

        // Should update both version fields
        assert!(staged.contains("version = \"2.0.0\""));
        assert!(!staged.contains("1.0.0"));
    }

    // README.md selective staging tests

    #[test]
    fn test_apply_readme_version_hunks_only_version_change() {
        let head = r#"# My Crate

Add to Cargo.toml:

```toml
my-crate = "0.1.0"
```
"#;
        let working = r#"# My Crate

Add to Cargo.toml:

```toml
my-crate = "0.2.0"
```
"#;

        let staged =
            apply_readme_version_hunks(head, working, "my-crate", "0.1.0", "0.2.0").unwrap();

        assert!(staged.contains(r#"my-crate = "0.2.0""#));
        assert!(!staged.contains(r#"my-crate = "0.1.0""#));
    }

    #[test]
    fn test_apply_readme_version_hunks_mixed_changes() {
        let head = r#"# My Crate

Old description.

```toml
my-crate = "0.1.0"
```
"#;
        let working = r#"# My Crate

New description with more details.

```toml
my-crate = "0.2.0"
```
"#;

        let staged =
            apply_readme_version_hunks(head, working, "my-crate", "0.1.0", "0.2.0").unwrap();

        // Should have version change
        assert!(staged.contains(r#"my-crate = "0.2.0""#));
        // Should NOT have description change - keeps old value
        assert!(staged.contains("Old description."));
        assert!(!staged.contains("New description"));
    }

    #[test]
    fn test_apply_readme_version_hunks_underscored_name() {
        let head = r#"my_crate = "1.0.0""#;
        let working = r#"my_crate = "1.1.0""#;

        let staged =
            apply_readme_version_hunks(head, working, "my-crate", "1.0.0", "1.1.0").unwrap();

        assert!(staged.contains(r#"my_crate = "1.1.0""#));
    }

    #[test]
    fn test_has_non_readme_version_changes_true() {
        let head = "# Readme\nmy-crate = \"0.1.0\"\n";
        let working = "# Updated Readme\nmy-crate = \"0.2.0\"\n";

        assert!(has_non_readme_version_changes(
            head, working, "my-crate", "0.1.0", "0.2.0"
        ));
    }

    #[test]
    fn test_has_non_readme_version_changes_false() {
        let head = "# Readme\nmy-crate = \"0.1.0\"\n";
        let working = "# Readme\nmy-crate = \"0.2.0\"\n";

        assert!(!has_non_readme_version_changes(
            head, working, "my-crate", "0.1.0", "0.2.0"
        ));
    }

    // Cargo.lock selective staging tests

    #[test]
    fn test_apply_cargo_lock_version_hunks_only_our_crate() {
        let head = r#"[[package]]
name = "my-crate"
version = "0.1.0"

[[package]]
name = "other-crate"
version = "1.0.0"
"#;
        let working = r#"[[package]]
name = "my-crate"
version = "0.2.0"

[[package]]
name = "other-crate"
version = "1.0.0"
"#;

        let staged =
            apply_cargo_lock_version_hunks(head, working, "my-crate", "0.1.0", "0.2.0").unwrap();

        assert!(staged.contains(r#"version = "0.2.0""#));
        assert!(!staged.contains(r#"version = "0.1.0""#));
    }

    #[test]
    fn test_apply_cargo_lock_version_hunks_for_all_workspace_members() {
        let head = r#"[[package]]
name = "workspace-root"
version = "0.20.1"

[[package]]
name = "workspace-member"
version = "0.20.1"

[[package]]
name = "registry-dependency"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        let working = r#"[[package]]
name = "workspace-root"
version = "0.20.2"

[[package]]
name = "workspace-member"
version = "0.20.2"

[[package]]
name = "registry-dependency"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        let workspace_packages =
            HashSet::from(["workspace-root".to_string(), "workspace-member".to_string()]);

        let staged = apply_workspace_cargo_lock_version_hunks(
            head,
            working,
            &workspace_packages,
            "0.20.1",
            "0.20.2",
        )
        .unwrap();

        assert_eq!(staged.matches(r#"version = "0.20.2""#).count(), 2);
        assert!(staged.contains("name = \"registry-dependency\"\nversion = \"1.0.0\""));
        assert!(!staged.contains("name = \"registry-dependency\"\nversion = \"2.0.0\""));
    }

    #[test]
    fn test_apply_cargo_lock_version_hunks_mixed_changes() {
        let head = r#"[[package]]
name = "my-crate"
version = "0.1.0"

[[package]]
name = "other-crate"
version = "1.0.0"
"#;
        let working = r#"[[package]]
name = "my-crate"
version = "0.2.0"

[[package]]
name = "other-crate"
version = "2.0.0"
"#;

        let staged =
            apply_cargo_lock_version_hunks(head, working, "my-crate", "0.1.0", "0.2.0").unwrap();

        // Should have our crate's version change
        assert!(staged.contains(r#"name = "my-crate""#));
        assert!(
            staged.contains(r#"version = "0.2.0""#),
            "Our crate's new version should be included"
        );

        // Should NOT have other-crate's version change - keeps old value
        assert!(staged.contains(r#"name = "other-crate""#));
        assert!(
            staged.contains(r#"version = "1.0.0""#),
            "Other crate's version should stay at 1.0.0"
        );
        assert!(
            !staged.matches(r#"version = "2.0.0""#).count() > 0
                || staged.matches(r#"version = "0.2.0""#).count() == 1,
            "Only our crate should have updated version"
        );
    }

    #[test]
    fn test_apply_cargo_lock_version_hunks_stale_head_version() {
        // Regression: HEAD's recorded version (0.0.15) is older than the
        // bump command's `old_version` (0.0.16). The structural splice
        // must replace the entire block, not just lines matching the
        // expected old/new strings.
        let head = r#"[[package]]
name = "my-crate"
version = "0.0.15"
dependencies = [
 "anyhow",
]

[[package]]
name = "other-crate"
version = "1.0.0"
"#;
        let working = r#"[[package]]
name = "my-crate"
version = "0.0.17"
dependencies = [
 "anyhow",
]

[[package]]
name = "other-crate"
version = "1.0.0"
"#;

        let staged =
            apply_cargo_lock_version_hunks(head, working, "my-crate", "0.0.16", "0.0.17").unwrap();

        assert_eq!(staged.matches(r#"version = "0.0.17""#).count(), 1);
        assert!(!staged.contains(r#"version = "0.0.15""#));
        assert!(!staged.contains(r#"version = "0.0.16""#));
    }

    #[test]
    fn test_has_non_cargo_lock_version_changes_true() {
        let head = r#"[[package]]
name = "my-crate"
version = "0.1.0"

[[package]]
name = "other-crate"
version = "1.0.0"
"#;
        let working = r#"[[package]]
name = "my-crate"
version = "0.2.0"

[[package]]
name = "other-crate"
version = "2.0.0"
"#;

        assert!(has_non_cargo_lock_version_changes(
            head, working, "my-crate", "0.1.0", "0.2.0"
        ));
    }

    #[test]
    fn test_has_non_cargo_lock_version_changes_false() {
        let head = r#"[[package]]
name = "my-crate"
version = "0.1.0"
"#;
        let working = r#"[[package]]
name = "my-crate"
version = "0.2.0"
"#;

        assert!(!has_non_cargo_lock_version_changes(
            head, working, "my-crate", "0.1.0", "0.2.0"
        ));
    }
}
