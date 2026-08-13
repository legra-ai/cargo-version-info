//! Git commit orchestration for version changes.
//!
//! This module coordinates the process of creating a git commit that contains
//! only version-related changes. This is the heart of the bump command's
//! "selective staging" functionality.
//!
//! # Selective Staging Rationale
//!
//! When bumping a version, we want to commit ONLY the version changes, not
//! any other uncommitted changes that might exist in the working directory.
//!
//! ## Why This Matters
//!
//! Consider this scenario:
//! ```text
//! Working Directory:
//! - Cargo.toml: version changed from 0.1.0 -> 0.1.1
//! - src/main.rs: work-in-progress feature (uncommitted)
//! - README.md: typo fixes (uncommitted)
//! ```
//!
//! We want the bump commit to include ONLY the Cargo.toml version change,
//! not the WIP feature or typo fixes. This keeps the version bump commit
//! clean and focused.
//!
//! # Commit Process
//!
//! The commit process involves several steps:
//!
//! 1. **Discover Repository**: Find the `.git` directory
//! 2. **Verify Changes**: Ensure version actually changed
//! 3. **Detect Other Changes**: Warn if non-version changes exist
//! 4. **Stage File**: Add file to git index
//! 5. **Build Tree**: Convert index to tree object
//! 6. **Create Commit**: Write commit object
//! 7. **Update HEAD**: Move current branch to new commit
//!
//! # Git Porcelain vs. Plumbing
//!
//! Git has two types of commands:
//!
//! ## Porcelain Commands (High-Level)
//! - `git add`, `git commit`, `git status`
//! - User-friendly, handle multiple operations
//! - What most people use day-to-day
//!
//! ## Plumbing Commands (Low-Level)
//! - `git hash-object`, `git update-index`, `git write-tree`
//! - Building blocks for porcelain commands
//! - What this module implements via `gix`
//!
//! By using gix's plumbing-level APIs, we have fine-grained control over
//! exactly what gets staged and committed.
//!
//! # Hunks and Patches
//!
//! In git terminology:
//! - **Hunk**: A contiguous block of changes in a file
//! - **Patch**: A collection of hunks (can span multiple files)
//!
//! Example diff with 2 hunks:
//! ```diff
//! @@ -1,3 +1,3 @@  ← Hunk 1: lines 1-3
//!  [package]
//!  name = "test"
//! -version = "0.1.0"
//! +version = "0.2.0"
//!  
//! @@ -10,2 +10,2 @@  ← Hunk 2: lines 10-11
//! -# Old comment
//! +# New comment
//! ```
//!
//! Our goal is to stage only the version hunk, not the comment hunk.
//!
//! # Current Implementation
//!
//! The current implementation stages the entire file if version changes are
//! detected, with a warning if non-version changes exist. This is simpler than
//! true hunk-level staging but works for the common case.
//!
//! ## Future Enhancement: True Hunk-Level Staging
//!
//! To implement true hunk-level staging, we would need to:
//!
//! 1. Generate a unified diff between HEAD and working directory
//! 2. Parse the diff into hunks
//! 3. Filter hunks to find only version-related changes
//! 4. Apply only those hunks to the index
//! 5. Build a tree from the partially-staged file
//!
//! This is complex because:
//! - Requires diff parsing and patch application
//! - Must handle merge conflicts in hunks
//! - Needs to update index with partial file content
//!
//! The git command `git add -p` (interactive patch mode) does this, but
//! implementing it programmatically is non-trivial.

use std::collections::HashSet;
use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
};
use bstr::ByteSlice;
use smallvec::SmallVec;

use super::diff;

/// Type of additional file for selective staging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// Cargo.lock file - filter to local workspace package changes
    CargoLock,
    /// README.md file - filter to only version reference changes
    Readme,
    /// Other files - commit full content
    Other,
}

/// An additional file to include in the version bump commit.
#[derive(Debug)]
pub struct AdditionalFile {
    /// Path to the file (absolute or relative to repo root)
    pub path: PathBuf,
    /// Current working directory content
    pub working_content: String,
    /// Content from HEAD (for selective staging). None means commit full
    /// content.
    pub head_content: Option<String>,
    /// Type of file (determines filtering strategy)
    pub file_type: FileType,
}

/// Commit version-related changes using pure gix (no git binary).
///
/// This function orchestrates the entire commit process:
/// - Discovers the git repository
/// - Verifies that version changes exist
/// - Warns about non-version changes
/// - Stages the file in the git index
/// - Builds a tree from the staged files
/// - Creates a commit object
/// - Updates the current branch reference
///
/// # Arguments
///
/// * `manifest_path` - Path to the Cargo.toml file (absolute or relative)
/// * `crate_name` - The crate/package name (for selective staging of related
///   files)
/// * `old_version` - The previous version (for verification and commit message)
/// * `new_version` - The new version (for verification and commit message)
///
/// # Errors
///
/// Returns an error if:
/// - Not in a git repository
/// - File doesn't have version changes
/// - Git operations fail (staging, tree building, commit creation)
/// - HEAD cannot be updated
///
/// # Examples
///
/// ```rust,no_run
/// # use std::path::Path;
/// # use anyhow::Result;
/// # fn example() -> Result<()> {
/// use cargo_version_info::commands::bump::commit::commit_version_changes;
///
/// let manifest = Path::new("./Cargo.toml");
/// commit_version_changes(manifest, "my-crate", "0.1.0", "0.2.0")?;
/// # Ok(())
/// # }
/// ```
///
/// # Implementation Details
///
/// ## Repository Discovery
///
/// Uses `gix::discover()` to find the repository by walking up from the
/// manifest directory. This handles cases where the manifest is in a
/// subdirectory of the repository.
///
/// ## Change Detection
///
/// We verify version changes by:
/// 1. Reading the file from HEAD commit
/// 2. Comparing it with the working directory version
/// 3. Checking if old_version appears in HEAD and new_version in working dir
///
/// This is a heuristic - we don't parse the TOML, just check for string
/// presence. This works reliably for version fields.
///
/// ## Non-Version Change Detection
///
/// To detect non-version changes, we:
/// 1. Split both versions into lines
/// 2. Compare line-by-line
/// 3. Flag differences that don't contain "version" or version numbers
///
/// This heuristic catches most non-version changes while avoiding false
/// positives from version-related formatting changes.
///
/// ## Staging Strategy
///
/// Currently stages the entire file. The steps are:
/// 1. Write file content as a blob object
/// 2. Create an index entry pointing to the blob
/// 3. Add entry to a new index state (removing old entry if present)
/// 4. Sort entries and write index to disk
///
/// ## Tree Building
///
/// Converts the flat index into a hierarchical tree structure. See the
/// [`tree`] module for details.
///
/// ## Commit Creation
///
/// Creates a commit object with:
/// - Tree: Built from the staged index
/// - Parents: Current HEAD commit
/// - Author/Committer: From git config or defaults
/// - Message: Conventional commit format "chore(version): bump X -> Y"
///
/// ## HEAD Update
///
/// Updates the current branch reference to point to the new commit. This is
/// equivalent to `git commit` moving the branch forward.
pub fn commit_version_changes(
    manifest_path: &Path,
    crate_name: &str,
    old_version: &str,
    new_version: &str,
) -> Result<()> {
    // Call the multi-file version with no additional files
    commit_version_changes_with_files(manifest_path, crate_name, old_version, new_version, &[])
}

/// Commit version-related changes along with additional files.
///
/// This function combines the selective staging of Cargo.toml (only version
/// changes) with selective staging for additional files like Cargo.lock
/// and README.md.
///
/// # Arguments
///
/// * `manifest_path` - Path to the Cargo.toml file
/// * `crate_name` - The crate/package name (for selective staging)
/// * `old_version` - The previous version
/// * `new_version` - The new version
/// * `additional_files` - List of additional files to include with their
///   content and metadata for selective staging
///
/// # Selective Staging
///
/// For all files, only version-related changes are committed:
///
/// - **Cargo.toml**: Only lines containing "version" or the version strings
/// - **Cargo.lock**: All local workspace package entries change
/// - **README.md**: Only lines with `crate-name = "version"` patterns
/// - **Other files**: Full content (no filtering)
///
/// This ensures that unrelated uncommitted changes (typo fixes, dependency
/// updates, etc.) are not accidentally included in the version bump commit.
pub fn commit_version_changes_with_files(
    manifest_path: &Path,
    crate_name: &str,
    old_version: &str,
    new_version: &str,
    additional_files: &[AdditionalFile],
) -> Result<()> {
    let workspace_package_names = workspace_package_names(manifest_path)?;

    // Discover git repository by walking up from the manifest's directory
    let repo = gix::discover(manifest_path.parent().unwrap_or_else(|| Path::new(".")))
        .context("Not in a git repository")?;

    // Calculate relative path from repository root
    // This is needed for index entries which use repo-relative paths
    let repo_path = repo.path().parent().context("Invalid repository path")?;
    let relative_path = manifest_path
        .strip_prefix(repo_path)
        .or_else(|_| manifest_path.strip_prefix("."))
        .unwrap_or(manifest_path);

    // Read current working directory content
    let current_content = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;

    // Get HEAD commit to compare against
    let head = repo.head().context("Failed to read HEAD")?;
    let head_commit_id = head.id().context("HEAD does not point to a commit")?;
    let head_commit = repo
        .find_object(head_commit_id)
        .context("Failed to find HEAD commit")?
        .try_into_commit()
        .context("HEAD is not a commit")?;

    // Get the tree from HEAD (what's currently committed)
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    // Verify that version changes exist
    verify_version_changes(
        &head_tree,
        relative_path,
        &current_content,
        old_version,
        new_version,
    )?;

    // Get HEAD content for comparison
    let head_content = get_head_content(&head_tree, relative_path)?;

    // Check if there are non-version changes in the file
    let has_other_changes =
        diff::has_non_version_changes(&head_content, &current_content, old_version, new_version);

    // Create the content to stage for Cargo.toml
    let staged_content = if has_other_changes {
        // File has non-version changes - apply only version hunks
        eprintln!("⚠️  Using hunk-level staging: only version lines will be committed.");

        // Apply only version-related hunks
        diff::apply_version_hunks(&head_content, &current_content, old_version, new_version)?
    } else {
        // File only has version changes - stage the whole file
        current_content.clone()
    };

    // Create blob for Cargo.toml
    let cargo_toml_blob_id = write_blob(&repo, &staged_content)?;

    // Build list of file updates for the tree
    let mut file_updates: Vec<(std::path::PathBuf, gix::ObjectId)> = Vec::new();
    file_updates.push((relative_path.to_path_buf(), cargo_toml_blob_id));

    // Add additional files (Cargo.lock, README.md, etc.) with selective staging
    for file in additional_files {
        let file_relative_path = file
            .path
            .strip_prefix(repo_path)
            .or_else(|_| file.path.strip_prefix("."))
            .unwrap_or(&file.path);

        // Determine what content to commit based on file type and available HEAD
        // content
        let content_to_commit = match (&file.head_content, file.file_type) {
            (Some(head_content), FileType::Readme) => {
                // Check if there are non-version changes
                if diff::has_non_readme_version_changes(
                    head_content,
                    &file.working_content,
                    crate_name,
                    old_version,
                    new_version,
                ) {
                    eprintln!(
                        "⚠️  Using hunk-level staging for README.md: only version lines will be committed."
                    );
                    diff::apply_readme_version_hunks(
                        head_content,
                        &file.working_content,
                        crate_name,
                        old_version,
                        new_version,
                    )?
                } else {
                    file.working_content.clone()
                }
            }
            (Some(head_content), FileType::CargoLock) => {
                // Check if there are non-version changes
                if diff::has_non_workspace_cargo_lock_version_changes(
                    head_content,
                    &file.working_content,
                    &workspace_package_names,
                    old_version,
                    new_version,
                ) {
                    eprintln!(
                        "⚠️  Using hunk-level staging for Cargo.lock: only workspace package versions will be committed."
                    );
                    diff::apply_workspace_cargo_lock_version_hunks(
                        head_content,
                        &file.working_content,
                        &workspace_package_names,
                        old_version,
                        new_version,
                    )?
                } else {
                    file.working_content.clone()
                }
            }
            _ => {
                // No HEAD content or Other file type - commit full content
                file.working_content.clone()
            }
        };

        let blob_id = write_blob(&repo, &content_to_commit)?;
        file_updates.push((file_relative_path.to_path_buf(), blob_id));
    }

    // Build tree with all updates
    let tree_id = update_tree_with_files(&repo, &head_tree, &file_updates)?;

    // Create the commit
    let commit_id = create_commit(&repo, &tree_id, head_commit_id, old_version, new_version)?;

    // Update HEAD to point to the new commit
    update_head(&repo, commit_id)?;

    // Reset the index to match HEAD
    // This is necessary because we created the commit via direct tree manipulation,
    // bypassing the index. Without this, any previously staged changes would remain
    // in the index, causing confusing `git status` output.
    reset_index_to_head(&repo)?;

    Ok(())
}

fn workspace_package_names(manifest_path: &Path) -> Result<HashSet<String>> {
    let mut command = cargo_metadata::MetadataCommand::new();
    command.manifest_path(manifest_path).no_deps();
    let metadata = command
        .exec()
        .context("Failed to read workspace metadata for lockfile staging")?;
    let workspace_members: HashSet<_> = metadata.workspace_members.iter().collect();

    Ok(metadata
        .packages
        .into_iter()
        .filter(|package| workspace_members.contains(&package.id))
        .map(|package| package.name.to_string())
        .collect())
}

/// Get the content of a file from the HEAD tree.
///
/// # Arguments
///
/// * `head_tree` - The tree from HEAD commit
/// * `relative_path` - Path to the file
///
/// # Returns
///
/// Returns the file content as a string.
///
/// # Errors
///
/// Returns an error if the file doesn't exist in HEAD or cannot be read.
fn get_head_content(head_tree: &gix::Tree, relative_path: &Path) -> Result<String> {
    let entry = head_tree
        .lookup_entry_by_path(relative_path)
        .context("Failed to lookup file in HEAD tree")?
        .context("File does not exist in HEAD")?;

    let blob = entry
        .object()
        .context("Failed to get blob from tree entry")?
        .try_into_blob()
        .context("Tree entry is not a blob")?;

    Ok(blob.data.to_str_lossy().into_owned())
}

/// Verify that the file has version-related changes.
///
/// Checks if:
/// - File exists in HEAD and old_version → new_version
/// - OR file is new and contains new_version
///
/// # Errors
///
/// Returns an error if no version-related changes are detected.
fn verify_version_changes(
    head_tree: &gix::Tree,
    relative_path: &Path,
    current_content: &str,
    old_version: &str,
    new_version: &str,
) -> Result<()> {
    let has_version_changes = if let Ok(Some(entry)) = head_tree.lookup_entry_by_path(relative_path)
    {
        // File exists in HEAD - verify version changed
        let head_blob = entry
            .object()
            .context("Failed to get blob from tree entry")?
            .try_into_blob()
            .context("Tree entry is not a blob")?;
        let head_content = head_blob.data.to_str_lossy();

        head_content.contains(old_version) && current_content.contains(new_version)
    } else {
        // File doesn't exist in HEAD - verify it's a version file
        current_content.contains("version") && current_content.contains(new_version)
    };

    if !has_version_changes {
        anyhow::bail!("No version-related changes found");
    }

    Ok(())
}

/// Write file content as a blob object to the git object database.
///
/// Git stores file contents as "blob" objects in `.git/objects/`. Each blob
/// is identified by the SHA-1 hash of its content.
///
/// # Arguments
///
/// * `repo` - The git repository
/// * `content` - The file content to store
///
/// # Returns
///
/// Returns the object ID (SHA-1 hash) of the blob.
fn write_blob(repo: &gix::Repository, content: &str) -> Result<gix::ObjectId> {
    let blob_id = repo
        .write_object(gix::objs::Blob {
            data: content.as_bytes().into(),
        })
        .context("Failed to write blob")?
        .detach();

    Ok(blob_id)
}

/// Update a tree by replacing multiple files' blobs, including nested paths.
///
/// Takes HEAD's tree and creates a NEW tree with the specified files changed.
/// All other files remain exactly as they were in HEAD.
///
/// This function handles nested paths by recursively updating subtrees.
/// For example, to update `npm/package.json`:
/// 1. Find the `npm` directory entry in the root tree
/// 2. Recursively update `package.json` within that subtree
/// 3. Replace the `npm` entry with the new subtree ID
///
/// # Arguments
///
/// * `repo` - The git repository
/// * `head_tree` - The tree from HEAD commit
/// * `file_updates` - List of (file_path, new_blob_id) pairs
fn update_tree_with_files(
    repo: &gix::Repository,
    head_tree: &gix::Tree,
    file_updates: &[(std::path::PathBuf, gix::ObjectId)],
) -> Result<gix::ObjectId> {
    use std::collections::HashMap;

    use gix::objs::{
        Tree,
        tree,
    };

    // Group updates by first path component
    // For "npm/package.json" -> key="npm", remaining="package.json"
    // For "Cargo.toml" -> key="Cargo.toml", remaining=None
    let mut grouped: HashMap<Vec<u8>, Vec<(Option<std::path::PathBuf>, gix::ObjectId)>> =
        HashMap::new();

    for (path, blob_id) in file_updates {
        let mut components = path.components();
        if let Some(first) = components.next() {
            let first_bytes = first.as_os_str().as_encoded_bytes().to_vec();
            let remaining: std::path::PathBuf = components.collect();
            let remaining = if remaining.as_os_str().is_empty() {
                None
            } else {
                Some(remaining)
            };
            grouped
                .entry(first_bytes)
                .or_default()
                .push((remaining, *blob_id));
        }
    }

    // Build new tree entries
    let mut tree_entries: Vec<tree::Entry> = Vec::new();

    for entry in head_tree.iter() {
        let entry = entry.context("Failed to iterate tree entry")?;
        let entry_name = entry.filename();

        if let Some(updates) = grouped.remove(entry_name.as_bytes()) {
            // This entry has updates - check if direct or nested
            let direct_updates: Vec<_> = updates
                .iter()
                .filter(|(remaining, _)| remaining.is_none())
                .collect();
            let nested_updates: Vec<_> = updates
                .iter()
                .filter_map(|(remaining, blob_id)| {
                    remaining.as_ref().map(|path| (path.clone(), *blob_id))
                })
                .collect();

            if !direct_updates.is_empty() {
                // Direct file update - use new blob (take the last one if multiple)
                let (_, blob_id) = direct_updates.last().unwrap();
                tree_entries.push(tree::Entry {
                    mode: entry.mode(),
                    filename: entry_name.into(),
                    oid: *blob_id,
                });
            } else if !nested_updates.is_empty() {
                // Updates are in subdirectory - recurse
                let subtree = entry
                    .object()
                    .context("Failed to get subtree object")?
                    .try_into_tree()
                    .context("Entry is not a tree")?;
                let new_subtree_id = update_tree_with_files(repo, &subtree, &nested_updates)?;
                tree_entries.push(tree::Entry {
                    mode: entry.mode(),
                    filename: entry_name.into(),
                    oid: new_subtree_id,
                });
            }
        } else {
            // No updates for this entry - keep unchanged
            tree_entries.push(tree::Entry {
                mode: entry.mode(),
                filename: entry_name.into(),
                oid: entry.oid().to_owned(),
            });
        }
    }

    // Sort entries using git's special sorting rules
    tree_entries.sort_by(|entry_a, entry_b| {
        use gix::objs::tree::EntryKind;

        let a_name = if matches!(entry_a.mode.kind(), EntryKind::Tree) {
            let mut name = entry_a.filename.to_vec();
            name.push(b'/');
            name
        } else {
            entry_a.filename.to_vec()
        };

        let b_name = if matches!(entry_b.mode.kind(), EntryKind::Tree) {
            let mut name = entry_b.filename.to_vec();
            name.push(b'/');
            name
        } else {
            entry_b.filename.to_vec()
        };

        a_name.cmp(&b_name)
    });

    // Build and write the tree
    let tree = Tree {
        entries: tree_entries,
    };

    let tree_id = repo
        .write_object(&tree)
        .context("Failed to write updated tree")?
        .detach();

    Ok(tree_id)
}

/// Create a commit object and write it to the object database.
///
/// # Git Commit Structure
///
/// A git commit is a simple text object containing:
/// ```text
/// tree <tree-sha1>
/// parent <parent-sha1>
/// author Name <email> timestamp timezone
/// committer Name <email> timestamp timezone
/// gpgsig -----BEGIN SSH SIGNATURE-----
///  <signature lines>
///  -----END SSH SIGNATURE-----
///
/// Commit message goes here
/// ```
///
/// # Signing
///
/// If signing is configured via git config (`commit.gpgsign = true`), the
/// commit will be signed using the configured method (SSH or GPG).
///
/// # Arguments
///
/// * `repo` - The git repository
/// * `tree_id` - The tree object ID (root tree of the commit)
/// * `parent_id` - The parent commit ID (current HEAD)
/// * `old_version` - Previous version (for commit message)
/// * `new_version` - New version (for commit message)
///
/// # Returns
///
/// Returns the object ID of the newly created commit.
fn create_commit(
    repo: &gix::Repository,
    tree_id: &gix::ObjectId,
    parent_id: gix::Id,
    old_version: &str,
    new_version: &str,
) -> Result<gix::ObjectId> {
    use super::signing;

    // Create commit message following conventional commits format
    let commit_message = format!("chore(version): bump {} -> {}", old_version, new_version);

    // Get author and committer from git config
    let author = get_signature_from_config(repo)?;
    let committer = author.clone();

    // Create parent list - commits can have multiple parents (for merges)
    // We only have one parent (the current HEAD)
    let parents: SmallVec<[gix::ObjectId; 1]> = SmallVec::from_iter([parent_id.detach()]);

    // Check if signing is configured
    let signing_config = signing::read_signing_config(repo);

    // Build extra headers for signature (if signing is enabled)
    let extra_headers = if signing_config.enabled {
        // Build the commit payload that will be signed
        let payload =
            signing::build_commit_payload(tree_id, parent_id, &author, &committer, &commit_message);

        // Sign the payload
        match signing::sign_commit_payload(&signing_config, &payload) {
            Ok(Some(signature)) => {
                // Add signature as gpgsig header
                vec![("gpgsig".into(), signature.into())]
            }
            Ok(None) => {
                // Signing not configured (shouldn't happen since we checked enabled)
                vec![]
            }
            Err(err) => {
                // Signing failed - this is an error, not a warning
                return Err(err.context("Failed to sign commit"));
            }
        }
    } else {
        vec![]
    };

    // Write the commit object to the object database
    let commit_id = repo
        .write_object(gix::objs::Commit {
            tree: *tree_id,
            parents,
            author,
            committer,
            message: commit_message.into(),
            encoding: None,
            extra_headers,
        })
        .context("Failed to write commit object")?
        .detach();

    Ok(commit_id)
}

/// Update HEAD to point to the new commit.
///
/// This moves the current branch forward to include the new commit. This is
/// equivalent to what `git commit` does after creating the commit object.
///
/// # Git References
///
/// HEAD can be:
/// - **Symbolic**: Points to a branch (e.g., `ref: refs/heads/main`)
/// - **Detached**: Points directly to a commit SHA
///
/// In normal operation, HEAD is symbolic and points to the current branch.
/// Updating HEAD in this case means updating the branch reference.
///
/// # Arguments
///
/// * `repo` - The git repository
/// * `commit_id` - The object ID of the commit to point HEAD to
///
/// # Errors
///
/// Returns an error if:
/// - HEAD doesn't exist or is invalid
/// - HEAD is not a reference (detached HEAD state)
/// - Reference update fails
fn update_head(repo: &gix::Repository, commit_id: gix::ObjectId) -> Result<()> {
    // Read current HEAD
    let mut head_ref = repo
        .head()
        .context("Failed to read HEAD")?
        .try_into_referent()
        .context("HEAD is not a reference (detached HEAD state)")?;

    // Update the reference to point to the new commit
    // This is an atomic operation - either succeeds completely or fails
    head_ref
        .set_target_id(commit_id, "bump version")
        .context("Failed to update HEAD reference")?;

    Ok(())
}

/// Reset the git index to match HEAD.
///
/// This is necessary after creating a commit via direct tree manipulation
/// (bypassing the index). Without this reset, any previously staged changes
/// would remain in the index, causing confusing `git status` output showing
/// staged changes that were already committed.
///
/// # How It Works
///
/// We read the tree from HEAD and write it to the index file. This is
/// equivalent to `git reset HEAD` (soft reset of the index only).
///
/// # Arguments
///
/// * `repo` - The git repository
///
/// # Errors
///
/// Returns an error if:
/// - HEAD cannot be read
/// - The tree cannot be accessed
/// - The index file cannot be written
fn reset_index_to_head(repo: &gix::Repository) -> Result<()> {
    // Get HEAD commit and its tree
    let mut head = repo.head().context("Failed to read HEAD")?;
    let head_commit = head
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    // Create a new index from the tree
    // Use default path validation options (allows most paths)
    let validate_opts = gix::validate::path::component::Options::default();
    let state = gix::index::State::from_tree(&head_tree.id, &repo.objects, validate_opts)
        .context("Failed to create index state from tree")?;

    let mut index = gix::index::File::from_state(state, repo.index_path());

    // Write the index to disk
    index
        .write(gix::index::write::Options::default())
        .context("Failed to write index")?;

    Ok(())
}

/// Get git signature (author/committer) from repository config.
///
/// Reads the `user.name` and `user.email` from git config and creates a
/// signature with the current timestamp.
///
/// # Required Configuration
///
/// This function REQUIRES that git config has both:
/// - `user.name` - The author's name
/// - `user.email` - The author's email
///
/// If either is missing, the function returns an error. This ensures commits
/// have proper attribution and prevents silent fallbacks that could lead to
/// incorrect author information.
///
/// # Setup Instructions
///
/// If you get an error about missing git config, set it with:
/// ```bash
/// git config user.name "Your Name"
/// git config user.email "your.email@example.com"
/// ```
///
/// Or globally:
/// ```bash
/// git config --global user.name "Your Name"
/// git config --global user.email "your.email@example.com"
/// ```
///
/// # Arguments
///
/// * `repo` - The git repository to read config from
///
/// # Returns
///
/// Returns a `Signature` with name, email, and current timestamp.
///
/// # Errors
///
/// Returns an error if:
/// - `user.name` is not set in git config
/// - `user.email` is not set in git config
/// - Config cannot be read
/// - Timestamp cannot be determined
fn get_signature_from_config(repo: &gix::Repository) -> Result<gix::actor::Signature> {
    let config = repo.config_snapshot();

    // Read user.name from config (REQUIRED - no fallback)
    let name = config
        .string("user.name")
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Git config 'user.name' is not set.\n\
                 Please configure it with:\n  \
                 git config user.name \"Your Name\""
            )
        })?;

    // Read user.email from config (REQUIRED - no fallback)
    let email = config
        .string("user.email")
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Git config 'user.email' is not set.\n\
                 Please configure it with:\n  \
                 git config user.email \"your.email@example.com\""
            )
        })?;

    // Get current time for the commit
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("Failed to get current time")?;

    let time = gix::date::Time {
        seconds: now.as_secs() as i64,
        offset: 0, // UTC
    };

    Ok(gix::actor::Signature {
        name: name.into(),
        email: email.into(),
        time,
    })
}
