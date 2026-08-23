//! Git index manipulation for staging files.
//!
//! This module handles the low-level git index operations required to stage
//! files for commit. The git index (also called the "staging area") is a binary
//! file that tracks which files should be included in the next commit.
//!
//! # Git Index Overview
//!
//! The git index serves as a staging area between the working directory and
//! the repository:
//!
//! ```text
//! Working Directory  →  Index (Staging)  →  Repository (Commits)
//!    (files on disk)      (.git/index)        (.git/objects/)
//! ```
//!
//! # Index Structure
//!
//! The index file contains:
//! - **Entries**: One per file, containing:
//!   - Path (stored in a shared string table for efficiency)
//!   - Object ID (SHA-1 of the file content)
//!   - Mode (file type: regular file, executable, symlink, etc.)
//!   - Stat info (timestamps, size, etc. for change detection)
//! - **Path Backing**: Shared storage for all entry paths
//! - **Extensions**: Optional metadata (tree cache, resolve undo, etc.)
//!
//! # Path Storage
//!
//! Paths in the index are stored in a unique way:
//! - All paths are stored in a single contiguous byte array (`path_backing`)
//! - Each entry's `path` field is a `Range<usize>` into this array
//! - This saves memory and makes the index more cache-friendly
//!
//! Example:
//! ```text
//! path_backing: b"Cargo.tomlsrc/main.rssrc/lib.rs"
//! entry[0].path: 0..10      ("Cargo.toml")
//! entry[1].path: 10..21     ("src/main.rs")
//! entry[2].path: 21..31     ("src/lib.rs")
//! ```
//!
//! # Why gix Instead of git Commands?
//!
//! Using `gix` (gitoxide) provides:
//! - **Type Safety**: Compile-time guarantees about git operations
//! - **Performance**: No process spawning overhead
//! - **Consistency**: Same API for all git operations
//! - **Testability**: Easier to mock and test
//!
//! # Staging Process
//!
//! To stage a file using `gix`:
//!
//! 1. **Load Index**: Read current index state from `.git/index`
//! 2. **Write Blob**: Store file content in git object database
//! 3. **Create Entry**: Build index entry with file metadata
//! 4. **Update State**: Add/update entry in index state
//! 5. **Sort Entries**: Maintain index invariant (sorted by path)
//! 6. **Write Index**: Save modified index back to disk
//!
//! # Challenges
//!
//! The gix index API is low-level and requires careful handling:
//! - Path storage must be managed manually
//! - Entries must be kept sorted
//! - The State struct doesn't allow direct mutation of path_backing
//! - We must rebuild the entire state to add entries with new paths

use std::path::Path;

use anyhow::{
    Context,
    Result,
};
use bstr::BStr;
use gix::index::{
    File,
    State,
    entry,
};

/// Stage a file in the git index.
///
/// This function adds or updates a file entry in the git index, making it
/// ready to be committed. It handles all the low-level details of:
/// - Loading the current index state
/// - Adding the file's path to the path backing storage
/// - Creating a properly formatted index entry
/// - Maintaining index invariants (sorted entries)
/// - Writing the updated index back to disk
///
/// # Arguments
///
/// * `index_path` - Path to the `.git/index` file
/// * `repo` - Reference to the git repository
/// * `relative_path` - Path to the file relative to repository root
/// * `blob_id` - Object ID of the file's content (already written to object db)
/// * `existing_state` - Current index state to modify
///
/// # Returns
///
/// Returns the new index state with the file staged.
///
/// # Errors
///
/// Returns an error if:
/// - The index file cannot be read or written
/// - The path contains invalid UTF-8
/// - Entries cannot be properly sorted
///
/// # Examples
///
/// ```rust,no_run
/// # use anyhow::Result;
/// # use gix::index::State;
/// # async fn example(repo: &gix::Repository) -> Result<State> {
/// use cargo_version_info::commands::bump::index::stage_file;
///
/// let index_path = repo.path().join("index");
/// let relative_path = std::path::Path::new("Cargo.toml");
/// let blob_id = gix::ObjectId::null(repo.object_hash());
/// let existing_state = State::new(repo.object_hash());
///
/// let new_state = stage_file(&index_path, repo, relative_path, blob_id, existing_state).await?;
/// # Ok(new_state)
/// # }
/// ```
///
/// # Implementation Details
///
/// ## Path Handling
///
/// Since the index stores paths in a shared backing array, we need to:
/// 1. Check if the path already exists (for updates)
/// 2. Create a new State to add the path properly
/// 3. Copy all existing entries (preserving their path references)
/// 4. Add the new entry with its path
///
/// ## Entry Creation
///
/// The `dangerously_push_entry` method is used because:
/// - It's the most efficient way to add entries
/// - The "dangerous" name indicates we must call `sort_entries()` afterward
/// - It handles path storage automatically
///
/// ## Sorting
///
/// Git requires index entries to be sorted by path. This is critical for:
/// - Binary search during status checks
/// - Merge operations
/// - Index format consistency
pub async fn stage_file(
    index_path: &Path,
    repo: &gix::Repository,
    relative_path: &Path,
    blob_id: gix::ObjectId,
    mut existing_state: State,
) -> Result<State> {
    // Find and remove existing entry for this path (if any)
    // This handles both new files and updates to existing files
    let path_bytes = relative_path.as_os_str().as_encoded_bytes();
    if let Some(pos) = existing_state
        .entries()
        .iter()
        .position(|e| e.path(&existing_state) == path_bytes)
    {
        // File already exists in index - remove old entry
        existing_state.remove_entry_at_index(pos);
    }

    // Create a new state with all entries including the updated one
    // We need to create a new state because we can't directly mutate path_backing
    let mut new_state = State::new(repo.object_hash());

    // Copy all existing entries to the new state
    // The dangerously_push_entry method handles path storage automatically
    for existing_entry in existing_state.entries() {
        let entry_path = existing_entry.path(&existing_state);
        new_state.dangerously_push_entry(
            existing_entry.stat,
            existing_entry.id,
            existing_entry.flags,
            existing_entry.mode,
            entry_path,
        );
    }

    // Add the new/updated entry
    // We use default stat since we've already verified the file has version changes
    // The stat is primarily used by git for optimization (detecting if file
    // changed)
    let path_bstr: &BStr = path_bytes.into();
    new_state.dangerously_push_entry(
        entry::Stat::default(),
        blob_id,
        entry::Flags::empty(),
        entry::Mode::FILE,
        path_bstr,
    );

    // Sort entries to maintain index integrity
    // This MUST be called after using dangerously_push_entry
    // Git requires entries to be sorted by path for binary search
    new_state.sort_entries();

    // Write the updated index back to disk
    let mut index_bytes = Vec::new();
    new_state
        .write_to(&mut index_bytes, gix::index::write::Options::default())
        .context("Failed to write index file")?;
    async_fs_io::write_bytes(index_path, &index_bytes)
        .await
        .context("Failed to write index file")?;

    Ok(new_state)
}

/// Load the current index state from disk.
///
/// This is a convenience wrapper around `gix::index::File::at()` that provides
/// better error messages and uses sensible defaults for the decode options.
///
/// # Arguments
///
/// * `index_path` - Path to the `.git/index` file
/// * `object_hash` - The hash algorithm used by the repository (sha1 or sha256)
///
/// # Returns
///
/// Returns the parsed index state.
///
/// # Errors
///
/// Returns an error if:
/// - The index file doesn't exist
/// - The index file is corrupted
/// - The index format is unsupported
///
/// # Examples
///
/// ```rust,no_run
/// # use anyhow::Result;
/// # async fn example(repo: &gix::Repository) -> Result<()> {
/// use cargo_version_info::commands::bump::index::load_index_state;
///
/// let index_path = repo.path().join("index");
/// let state = load_index_state(&index_path, repo.object_hash())?;
///
/// println!("Index has {} entries", state.entries().len());
/// # Ok(())
/// # }
/// ```
pub fn load_index_state(index_path: &Path, object_hash: gix::hash::Kind) -> Result<State> {
    let file = File::at(
        index_path,
        object_hash,
        false, // skip_hash: don't verify checksums (faster for reading)
        gix::index::decode::Options::default(),
    )
    .context("Failed to read index file")?;

    Ok(State::from(file))
}
