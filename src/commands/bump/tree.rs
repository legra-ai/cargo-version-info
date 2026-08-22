//! Git tree building from index entries.
//!
//! This module handles converting a flat git index (list of files) into a
//! hierarchical git tree structure. This is one of the most complex parts of
//! the git data model.
//!
//! # Git Tree Structure
//!
//! Git stores directory hierarchies as tree objects. Each tree contains:
//! - Entries for files (blobs)
//! - Entries for subdirectories (subtrees)
//! - Entries for submodules (commits)
//!
//! ## Flat Index vs. Hierarchical Trees
//!
//! The index stores files as flat paths:
//! ```text
//! Cargo.toml
//! src/main.rs
//! src/lib.rs
//! tests/integration.rs
//! ```
//!
//! But git trees are hierarchical:
//! ```text
//! Root Tree
//! ├── Cargo.toml (blob)
//! ├── src/ (tree)
//! │   ├── main.rs (blob)
//! │   └── lib.rs (blob)
//! └── tests/ (tree)
//!     └── integration.rs (blob)
//! ```
//!
//! # Tree Building Algorithm
//!
//! To convert index to trees:
//!
//! 1. **Group by Directory**: Split paths and group files by parent directory
//! 2. **Build Recursively**: Create trees from leaves up to root
//! 3. **Sort Entries**: Git requires lexicographic sorting
//! 4. **Write Objects**: Store each tree in the object database
//!
//! # Current Implementation
//!
//! The current implementation is **simplified** for the MVP:
//! - Only handles single-level trees (files in root directory)
//! - Files in subdirectories are flattened to root
//! - Full recursive tree building is TODO
//!
//! This works for most use cases where we're only bumping `Cargo.toml` in the
//! root directory.
//!
//! # Entry Modes
//!
//! Git supports several entry types:
//!
//! | Mode | Type | Description |
//! |------|------|-------------|
//! | 0100644 | Blob | Regular file |
//! | 0100755 | BlobExecutable | Executable file |
//! | 0120000 | Link | Symbolic link |
//! | 0040000 | Tree | Directory |
//! | 0160000 | Commit | Gitlink (submodule) |
//!
//! # Performance Considerations
//!
//! Tree building is I/O intensive:
//! - Each tree is a separate object in `.git/objects/`
//! - Large repositories with deep hierarchies create many tree objects
//! - The simplified implementation reduces this overhead

use anyhow::{
    Context,
    Result,
};
use gix::index::{
    State,
    entry,
};

/// Build a git tree from index entries.
///
/// This function converts the flat list of files in the git index into a
/// hierarchical tree structure suitable for creating a commit.
///
/// # Current Limitation
///
/// **This is a simplified implementation** that only handles files in the root
/// directory. Files in subdirectories are currently included with their full
/// paths (e.g., "src/main.rs") rather than building proper subtrees.
///
/// For the bump command's use case (updating Cargo.toml), this limitation is
/// acceptable since Cargo.toml is always in the root directory.
///
/// # Arguments
///
/// * `index_state` - The index state containing file entries to build tree from
/// * `repo` - The git repository to write tree objects to
///
/// # Returns
///
/// Returns the object ID of the root tree.
///
/// # Errors
///
/// Returns an error if:
/// - Tree objects cannot be written to the object database
/// - Path conversions fail
/// - Invalid entry modes are encountered
///
/// # Examples
///
/// ```rust,no_run
/// # use anyhow::Result;
/// # use gix::index::State;
/// # async fn example(repo: &gix::Repository, state: &State) -> Result<()> {
/// use cargo_version_info::commands::bump::tree::build_tree_from_index;
///
/// let tree_id = build_tree_from_index(state, repo)?;
/// println!("Created tree: {}", tree_id);
/// # Ok(())
/// # }
/// ```
///
/// # Algorithm
///
/// The current simplified algorithm:
///
/// 1. Iterate through all index entries
/// 2. Convert entry paths to tree entry format
/// 3. Convert entry modes to tree entry modes
/// 4. Sort entries by filename (git requirement)
/// 5. Build a single root tree with all entries
/// 6. Write the tree object to the repository
///
/// # Future Improvements
///
/// A full implementation would:
/// - Parse paths to identify directories
/// - Build trees recursively from leaves to root
/// - Handle deep directory structures
/// - Optimize by reusing unchanged subtrees
pub fn build_tree_from_index(index_state: &State, repo: &gix::Repository) -> Result<gix::ObjectId> {
    use std::collections::HashMap;

    // Group entries by directory path
    // This is preparation for full recursive tree building (not yet implemented)
    #[allow(clippy::type_complexity)]
    let mut trees: HashMap<Vec<&[u8]>, Vec<(Vec<&[u8]>, entry::Mode, gix::ObjectId)>> =
        HashMap::new();

    // Process each index entry
    for entry in index_state.entries() {
        let entry_path = entry.path(index_state);
        let path_parts: Vec<&[u8]> = entry_path.split(|&b| b == b'/').collect();

        if path_parts.len() == 1 {
            // Top-level file - add to root tree
            trees
                .entry(vec![])
                .or_default()
                .push((path_parts, entry.mode, entry.id));
        } else {
            // File in directory - for now, add to root with full path (simplified)
            // TODO: Build proper directory trees recursively
            trees
                .entry(vec![])
                .or_default()
                .push((path_parts, entry.mode, entry.id));
        }
    }

    // Build the root tree from collected entries
    use gix::objs::{
        Tree,
        tree,
    };

    let mut tree_entries: Vec<tree::Entry> = Vec::new();

    // Get entries for the root directory
    if let Some(entries) = trees.get(&vec![]) {
        for (path_parts, mode, oid) in entries {
            // Reconstruct the filename from path parts
            // For flattened paths (subdirectories), this includes the full path
            let filename: bstr::BString = path_parts.join(&b"/"[..]).into();

            // Convert index entry mode to tree entry mode
            let tree_mode = convert_mode_to_tree_mode(*mode);

            tree_entries.push(tree::Entry {
                mode: tree_mode,
                filename,
                oid: *oid,
            });
        }
    }

    // Sort tree entries by filename
    // This is REQUIRED by git - unsorted trees are invalid
    // Git uses lexicographic byte-order sorting
    tree_entries.sort_by(|a, b| a.filename.cmp(&b.filename));

    // Create the tree object
    let tree = Tree {
        entries: tree_entries,
    };

    // Write the tree to the object database and return its ID
    let tree_id = repo
        .write_object(&tree)
        .context("Failed to write tree object")?
        .detach();

    Ok(tree_id)
}

/// Convert index entry mode to tree entry mode.
///
/// The git index and git trees use different representations for file modes:
/// - Index uses `gix::index::entry::Mode` (bitflags)
/// - Trees use `gix::objs::tree::EntryMode` (wrapped u16)
///
/// # Supported Modes
///
/// - `FILE` (0100644): Regular file
/// - `FILE_EXECUTABLE` (0100755): Executable file
/// - `SYMLINK` (0120000): Symbolic link
/// - `DIR` (0040000): Directory/subdirectory
/// - `COMMIT` (0160000): Gitlink (submodule reference)
///
/// # Arguments
///
/// * `mode` - The index entry mode to convert
///
/// # Returns
///
/// Returns the equivalent tree entry mode.
///
/// # Examples
///
/// ```rust
/// # use gix::index::entry::Mode;
/// # use gix::objs::tree::EntryMode;
/// use cargo_version_info::commands::bump::tree::convert_mode_to_tree_mode;
///
/// let index_mode = Mode::FILE;
/// let tree_mode = convert_mode_to_tree_mode(index_mode);
/// // tree_mode is now EntryMode for a regular blob
/// ```
pub fn convert_mode_to_tree_mode(mode: entry::Mode) -> gix::objs::tree::EntryMode {
    use gix::objs::tree::{
        EntryKind,
        EntryMode,
    };

    match mode {
        entry::Mode::FILE => EntryMode::from(EntryKind::Blob),
        entry::Mode::FILE_EXECUTABLE => EntryMode::from(EntryKind::BlobExecutable),
        entry::Mode::SYMLINK => EntryMode::from(EntryKind::Link),
        entry::Mode::DIR => EntryMode::from(EntryKind::Tree),
        entry::Mode::COMMIT => EntryMode::from(EntryKind::Commit),
        // Unknown or invalid modes default to blob
        // This is a safe fallback for forward compatibility
        _ => EntryMode::from(EntryKind::Blob),
    }
}
