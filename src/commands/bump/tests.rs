//! Tests for the bump command.
//!
//! This module contains comprehensive tests for all aspects of the bump
//! command including version calculation, TOML updates, and git integration.

use bstr::ByteSlice;
use tempfile::TempDir;

use super::*;

/// Create a temporary cargo project for testing.
///
/// Creates a minimal valid cargo project with:
/// - Cargo.toml with the specified content
/// - src/ directory
/// - src/lib.rs with minimal content (required by cargo_metadata)
fn create_temp_cargo_project(content: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("Cargo.toml");
    std::fs::write(&manifest_path, content).unwrap();

    // Create src directory with a minimal lib.rs for cargo metadata to work
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    dir
}

/// Initialize a git repository in the test directory.
///
/// Uses git commands for test setup (simpler and more reliable than using gix
/// for initialization). The important part is that the bump function itself
/// uses gix, not the test setup.
fn init_test_git_repo(dir: &std::path::Path) {
    std::process::Command::new("git")
        .arg("init")
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .output()
        .unwrap();
    // Disable commit signing in tests to avoid dependency on SSH keys
    std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "Cargo.toml"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
#[serial_test::serial]
fn test_bump_patch_version() {
    let dir = create_temp_cargo_project(
        r#"
[package]
name = "test"
version = "0.1.2"
"#,
    );
    let manifest_path = dir.path().join("Cargo.toml");

    init_test_git_repo(dir.path());

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        version: None,
        auto: false,
        major: false,
        minor: false,
        patch: true,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true, // Don't commit in tests
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok());

    // Verify version was updated
    let content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(content.contains("version = \"0.1.3\""));
}

#[test]
#[serial_test::serial]
fn test_bump_minor_version() {
    let dir = create_temp_cargo_project(
        r#"
[package]
name = "test"
version = "0.1.2"
"#,
    );
    let manifest_path = dir.path().join("Cargo.toml");

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        version: None,
        auto: false,
        major: false,
        minor: true,
        patch: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(content.contains("version = \"0.2.0\""));
}

#[test]
#[serial_test::serial]
fn test_bump_major_version() {
    let dir = create_temp_cargo_project(
        r#"
[package]
name = "test"
version = "0.1.2"
"#,
    );
    let manifest_path = dir.path().join("Cargo.toml");

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        version: None,
        auto: false,
        major: true,
        minor: false,
        patch: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(content.contains("version = \"1.0.0\""));
}

#[test]
#[serial_test::serial]
fn test_bump_manual_version() {
    let dir = create_temp_cargo_project(
        r#"
[package]
name = "test"
version = "0.1.2"
"#,
    );
    let manifest_path = dir.path().join("Cargo.toml");

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        version: Some("2.5.10".to_string()),
        auto: false,
        major: false,
        minor: false,
        patch: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(content.contains("version = \"2.5.10\""));
}

#[test]
#[serial_test::serial]
fn test_bump_same_version_error() {
    let dir = create_temp_cargo_project(
        r#"
[package]
name = "test"
version = "0.1.2"
"#,
    );
    let manifest_path = dir.path().join("Cargo.toml");

    let args = BumpArgs {
        manifest_path: Some(manifest_path),
        version: Some("0.1.2".to_string()),
        auto: false,
        major: false,
        minor: false,
        patch: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("already the target version")
    );
}

/// Create a test git repository using gix (not git commands).
///
/// This creates a proper git repository with:
/// - Initial commit containing Cargo.toml
/// - Proper author/committer configuration
/// - Ready for testing bump operations
fn create_test_git_repo_with_gix(dir: &std::path::Path, initial_content: &str) -> gix::Repository {
    use gix::index::{
        State,
        entry,
    };
    use smallvec::SmallVec;

    // Initialize repository
    let repo = gix::init(dir).expect("Failed to initialize git repository");

    // Create Cargo.toml
    let manifest_path = dir.join("Cargo.toml");
    std::fs::write(&manifest_path, initial_content).expect("Failed to write Cargo.toml");

    // Create src/lib.rs for valid cargo project
    let src_dir = dir.join("src");
    std::fs::create_dir_all(&src_dir).expect("Failed to create src directory");
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").expect("Failed to write lib.rs");

    // Create initial commit using gix
    // 1. Create empty index
    let mut index_state = State::new(repo.object_hash());

    // 2. Add Cargo.toml to index
    let cargo_toml_blob = repo
        .write_object(gix::objs::Blob {
            data: initial_content.as_bytes().into(),
        })
        .expect("Failed to write Cargo.toml blob")
        .detach();

    let cargo_path: &bstr::BStr = b"Cargo.toml".into();
    index_state.dangerously_push_entry(
        entry::Stat::default(),
        cargo_toml_blob,
        entry::Flags::empty(),
        entry::Mode::FILE,
        cargo_path,
    );

    // 3. Add src/lib.rs to index
    let lib_rs_blob = repo
        .write_object(gix::objs::Blob {
            data: b"// Test library\n".into(),
        })
        .expect("Failed to write lib.rs blob")
        .detach();

    let lib_path: &bstr::BStr = b"src/lib.rs".into();
    index_state.dangerously_push_entry(
        entry::Stat::default(),
        lib_rs_blob,
        entry::Flags::empty(),
        entry::Mode::FILE,
        lib_path,
    );

    index_state.sort_entries();

    // 4. Build tree from index
    use gix::objs::{
        Tree,
        tree,
    };

    // Create src/ subtree
    let src_tree = Tree {
        entries: vec![tree::Entry {
            mode: tree::EntryMode::from(tree::EntryKind::Blob),
            filename: b"lib.rs".into(),
            oid: lib_rs_blob,
        }],
    };
    let src_tree_id = repo
        .write_object(&src_tree)
        .expect("Failed to write src tree")
        .detach();

    // Create root tree
    let root_tree = Tree {
        entries: vec![
            tree::Entry {
                mode: tree::EntryMode::from(tree::EntryKind::Blob),
                filename: b"Cargo.toml".into(),
                oid: cargo_toml_blob,
            },
            tree::Entry {
                mode: tree::EntryMode::from(tree::EntryKind::Tree),
                filename: b"src".into(),
                oid: src_tree_id,
            },
        ],
    };
    let tree_id = repo
        .write_object(&root_tree)
        .expect("Failed to write root tree")
        .detach();

    // 5. Create initial commit
    let author = gix::actor::Signature {
        name: "Test User".into(),
        email: "test@example.com".into(),
        time: gix::date::Time {
            seconds: 1234567890,
            offset: 0,
        },
    };

    let commit = gix::objs::Commit {
        tree: tree_id,
        parents: SmallVec::new(),
        author: author.clone(),
        committer: author,
        message: "Initial commit".into(),
        encoding: None,
        extra_headers: vec![],
    };
    let commit_id = repo
        .write_object(&commit)
        .expect("Failed to write commit")
        .detach();

    // 6. Create and update main branch
    repo.refs
        .transaction()
        .prepare(
            vec![gix::refs::transaction::RefEdit {
                change: gix::refs::transaction::Change::Update {
                    log: gix::refs::transaction::LogChange {
                        mode: gix::refs::transaction::RefLog::AndReference,
                        force_create_reflog: false,
                        message: "initial commit".into(),
                    },
                    expected: gix::refs::transaction::PreviousValue::Any,
                    new: gix::refs::Target::Object(commit_id),
                },
                name: "refs/heads/main".try_into().expect("Invalid ref name"),
                deref: false,
            }],
            gix::lock::acquire::Fail::Immediately,
            gix::lock::acquire::Fail::Immediately,
        )
        .expect("Failed to prepare transaction")
        .commit(Some(gix::actor::SignatureRef {
            name: "Test User".into(),
            email: "test@example.com".into(),
            time: "1234567890 +0000",
        }))
        .expect("Failed to commit transaction");

    // Set HEAD to point to refs/heads/main using a separate transaction
    // HEAD must point to a branch for bump to work correctly
    let main_ref_name: gix::refs::FullName =
        "refs/heads/main".try_into().expect("Invalid ref name");
    repo.refs
        .transaction()
        .prepare(
            vec![gix::refs::transaction::RefEdit {
                change: gix::refs::transaction::Change::Update {
                    log: gix::refs::transaction::LogChange {
                        mode: gix::refs::transaction::RefLog::AndReference,
                        force_create_reflog: false,
                        message: "initial commit".into(),
                    },
                    expected: gix::refs::transaction::PreviousValue::Any,
                    new: gix::refs::Target::Symbolic(main_ref_name),
                },
                name: "HEAD".try_into().expect("Invalid ref name"),
                deref: false,
            }],
            gix::lock::acquire::Fail::Immediately,
            gix::lock::acquire::Fail::Immediately,
        )
        .expect("Failed to prepare HEAD transaction")
        .commit(Some(gix::actor::SignatureRef {
            name: "Test User".into(),
            email: "test@example.com".into(),
            time: "1234567890 +0000",
        }))
        .expect("Failed to commit HEAD transaction");

    // Set user.name and user.email in repo config for bump command
    // Also disable commit signing to avoid dependency on SSH keys in tests
    let config_path = repo.path().join("config");
    let config_content = std::fs::read_to_string(&config_path).unwrap_or_else(|_| String::new());
    let new_config = format!(
        "{}\n[user]\n\tname = Test User\n\temail = test@example.com\n[commit]\n\tgpgsign = false\n",
        config_content
    );
    std::fs::write(&config_path, new_config).expect("Failed to write config");

    repo
}

#[test]
#[serial_test::serial]
fn test_hunk_level_staging_only_version_line() {
    // Create repo with initial content
    let dir = tempfile::tempdir().unwrap();
    let initial_content = r#"[package]
name = "test"
version = "0.1.0"
description = "original description"
edition = "2021"
"#;

    let _repo = create_test_git_repo_with_gix(dir.path(), initial_content);

    // Modify Cargo.toml: change version AND description
    let manifest_path = dir.path().join("Cargo.toml");
    let modified_content = r#"[package]
name = "test"
version = "0.1.0"
description = "modified description"
edition = "2021"
"#;
    std::fs::write(&manifest_path, modified_content).expect("Failed to modify Cargo.toml");

    // Run bump command
    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        version: Some("0.2.0".to_string()),
        auto: false,
        major: false,
        minor: false,
        patch: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false, // DO commit
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify the commit using gix
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    // Get the tree from the commit
    let tree = commit.tree().expect("Failed to get tree");

    // Get Cargo.toml from the commit
    let cargo_entry = tree
        .lookup_entry_by_path("Cargo.toml")
        .expect("Failed to lookup Cargo.toml")
        .expect("Cargo.toml not in commit");

    let blob = cargo_entry
        .object()
        .expect("Failed to get blob")
        .try_into_blob()
        .expect("Not a blob");

    let committed_content = blob.data.to_str_lossy();

    // Verify ONLY version line changed
    assert!(
        committed_content.contains("version = \"0.2.0\""),
        "Version should be updated in commit"
    );
    assert!(
        committed_content.contains("description = \"original description\""),
        "Description should NOT be changed in commit (should be original)"
    );
    assert!(
        !committed_content.contains("description = \"modified description\""),
        "Modified description should NOT be in commit"
    );

    // Verify working directory still has the description change
    let working_content = std::fs::read_to_string(&manifest_path).expect("Failed to read file");
    assert!(
        working_content.contains("description = \"modified description\""),
        "Working directory should still have modified description"
    );
}

#[test]
#[serial_test::serial]
fn test_hunk_level_staging_multiple_changes() {
    // Test with multiple non-version changes
    let dir = tempfile::tempdir().unwrap();
    let initial_content = r#"[package]
name = "test"
version = "1.0.0"
authors = ["Original Author"]
description = "A test crate"
license = "MIT"
"#;

    let _repo = create_test_git_repo_with_gix(dir.path(), initial_content);

    // Modify multiple fields including version
    let manifest_path = dir.path().join("Cargo.toml");
    let modified_content = r#"[package]
name = "test"
version = "1.0.0"
authors = ["New Author"]
description = "An updated test crate"
license = "Apache-2.0"
"#;
    std::fs::write(&manifest_path, modified_content).expect("Failed to modify Cargo.toml");

    // Run bump to change version
    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify the commit
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    let tree = commit.tree().expect("Failed to get tree");
    let cargo_entry = tree
        .lookup_entry_by_path("Cargo.toml")
        .expect("Failed to lookup Cargo.toml")
        .expect("Cargo.toml not in commit");

    let blob = cargo_entry
        .object()
        .expect("Failed to get blob")
        .try_into_blob()
        .expect("Not a blob");

    let committed_content = blob.data.to_str_lossy();

    // Verify ONLY version changed
    assert!(
        committed_content.contains("version = \"1.0.1\""),
        "Version should be bumped to 1.0.1"
    );
    assert!(
        committed_content.contains("authors = [\"Original Author\"]"),
        "Authors should be original, not modified"
    );
    assert!(
        committed_content.contains("description = \"A test crate\""),
        "Description should be original, not modified"
    );
    assert!(
        committed_content.contains("license = \"MIT\""),
        "License should be original, not modified"
    );

    // Verify working directory still has all the other changes
    let working_content = std::fs::read_to_string(&manifest_path).expect("Failed to read file");
    assert!(working_content.contains("authors = [\"New Author\"]"));
    assert!(working_content.contains("description = \"An updated test crate\""));
    assert!(working_content.contains("license = \"Apache-2.0\""));
}

#[test]
#[serial_test::serial]
fn test_commit_has_proper_author() {
    // Verify commits have proper author from git config
    let dir = tempfile::tempdir().unwrap();
    let initial_content = r#"[package]
name = "test"
version = "0.5.0"
"#;

    let _repo = create_test_git_repo_with_gix(dir.path(), initial_content);

    let manifest_path = dir.path().join("Cargo.toml");

    // Run bump
    let args = BumpArgs {
        manifest_path: Some(manifest_path),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify the commit has proper author
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    // Check author
    let author = commit.author().expect("Failed to get author");
    assert_eq!(
        author.name.to_string(),
        "Test User",
        "Author name should be set"
    );
    assert_eq!(
        author.email.as_bstr(),
        "test@example.com",
        "Author email should be set"
    );
    // Check that time is set (not empty)
    assert!(!author.time.is_empty(), "Author time should not be empty");

    // Check committer
    let committer = commit.committer().expect("Failed to get committer");
    assert_eq!(
        committer.name.to_string(),
        "Test User",
        "Committer name should be set"
    );
    assert_eq!(
        committer.email.to_string(),
        "test@example.com",
        "Committer email should be set"
    );
}

// Skip on Windows due to gix file locking issues
#[test]
#[serial_test::serial]
#[cfg(unix)]
fn test_only_version_file_in_commit_not_other_staged_files() {
    // Verify that bump doesn't include other staged files
    let dir = tempfile::tempdir().unwrap();
    let initial_content = r#"[package]
name = "test"
version = "2.0.0"
"#;

    let repo = create_test_git_repo_with_gix(dir.path(), initial_content);

    // Create another file and stage it (but don't commit)
    let readme_path = dir.path().join("README.md");
    std::fs::write(&readme_path, "# Test Project\n").expect("Failed to write README");

    // Stage the README using gix
    let index_path = repo.path().join("index");

    use gix::index::{
        File,
        State,
        entry,
    };

    // Create or load index
    let mut index_state = if index_path.exists() {
        let file = File::at(
            &index_path,
            repo.object_hash(),
            false,
            gix::index::decode::Options::default(),
        )
        .expect("Failed to read index");
        State::from(file)
    } else {
        // Index doesn't exist yet, create empty one
        State::new(repo.object_hash())
    };

    // Add README.md to index
    let readme_blob = repo
        .write_object(gix::objs::Blob {
            data: b"# Test Project\n".into(),
        })
        .expect("Failed to write README blob")
        .detach();

    let readme_path_bstr: &bstr::BStr = b"README.md".into();
    index_state.dangerously_push_entry(
        entry::Stat::default(),
        readme_blob,
        entry::Flags::empty(),
        entry::Mode::FILE,
        readme_path_bstr,
    );
    index_state.sort_entries();

    // Write index back to disk (staging README.md)
    let mut index_file_write =
        std::fs::File::create(&index_path).expect("Failed to create index file");
    index_state
        .write_to(&mut index_file_write, gix::index::write::Options::default())
        .expect("Failed to write index");

    // Now run bump - it should NOT include README.md
    let manifest_path = dir.path().join("Cargo.toml");
    let args = BumpArgs {
        manifest_path: Some(manifest_path),
        major: true,
        version: None,
        auto: false,
        minor: false,
        patch: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify the commit does NOT contain README.md
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    let tree = commit.tree().expect("Failed to get tree");

    // Verify Cargo.toml is in the commit
    assert!(
        tree.lookup_entry_by_path("Cargo.toml")
            .expect("Failed to lookup")
            .is_some(),
        "Cargo.toml should be in commit"
    );

    // Verify README.md is NOT in the commit
    assert!(
        tree.lookup_entry_by_path("README.md")
            .expect("Failed to lookup")
            .is_none(),
        "README.md should NOT be in commit (was staged but not committed by bump)"
    );

    // The key assertion passed: README.md was staged but NOT included in the
    // bump commit. This proves the bump command creates a minimal index with
    // only the version file, regardless of what's in .git/index.
}

#[test]
#[serial_test::serial]
fn test_preserves_all_files_from_head() {
    // CRITICAL REGRESSION TEST:
    // Verify that bump doesn't delete other files by creating a minimal tree.
    // This is the bug that caused all files to be deleted in commit 7192f12.

    let dir = tempfile::tempdir().unwrap();
    let initial_content = r#"[package]
name = "test"
version = "1.0.0"
"#;

    let _repo = create_test_git_repo_with_gix(dir.path(), initial_content);

    // The initial commit has:
    // - Cargo.toml
    // - src/lib.rs (created by create_test_git_repo_with_gix)
    // Both should be in the bump commit!

    // Run bump
    let manifest_path = dir.path().join("Cargo.toml");
    let args = BumpArgs {
        manifest_path: Some(manifest_path),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify the commit using gix
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    let tree = commit.tree().expect("Failed to get tree");

    // CRITICAL: Verify Cargo.toml is in the commit
    assert!(
        tree.lookup_entry_by_path("Cargo.toml")
            .expect("Failed to lookup")
            .is_some(),
        "Cargo.toml should be in commit"
    );

    // CRITICAL: Verify src/lib.rs is STILL in the commit (not deleted!)
    let src_entry = tree
        .lookup_entry_by_path("src/lib.rs")
        .expect("Failed to lookup src/lib.rs");

    assert!(
        src_entry.is_some(),
        "src/lib.rs should still be in commit - bump should preserve all files from HEAD!"
    );

    // Verify src/lib.rs content is unchanged
    if let Some(entry) = src_entry {
        let blob = entry
            .object()
            .expect("Failed to get blob")
            .try_into_blob()
            .expect("Not a blob");

        let content = blob.data.to_str_lossy();
        assert_eq!(
            content, "// Test library\n",
            "src/lib.rs content should be unchanged"
        );
    }

    // Verify Cargo.toml version was updated
    let cargo_entry = tree
        .lookup_entry_by_path("Cargo.toml")
        .expect("Failed to lookup")
        .expect("Cargo.toml not in tree");

    let cargo_blob = cargo_entry
        .object()
        .expect("Failed to get blob")
        .try_into_blob()
        .expect("Not a blob");

    let cargo_content = cargo_blob.data.to_str_lossy();
    assert!(
        cargo_content.contains("version = \"1.0.1\""),
        "Cargo.toml version should be bumped"
    );
}

#[test]
#[serial_test::serial]
fn test_preserves_multiple_files_and_directories() {
    // Extended regression test: verify bump preserves complex directory structures
    let dir = tempfile::tempdir().unwrap();
    let initial_content = r#"[package]
name = "multi-file-test"
version = "0.5.0"
"#;

    let _repo = create_test_git_repo_with_gix(dir.path(), initial_content);

    // Add more files to the initial commit
    // Create additional files: README.md, .gitignore, docs/guide.md
    std::fs::write(dir.path().join("README.md"), "# Project\n").expect("Failed to write README");
    std::fs::write(dir.path().join(".gitignore"), "target/\n").expect("Failed to write .gitignore");

    let docs_dir = dir.path().join("docs");
    std::fs::create_dir_all(&docs_dir).expect("Failed to create docs dir");
    std::fs::write(docs_dir.join("guide.md"), "# Guide\n").expect("Failed to write guide");

    // Build a tree with all files
    // For simplicity, we'll use git commands to add these files
    // (the test is about bump, not about our tree building)
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .expect("Failed to git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "Add more files"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to git commit");

    // Now run bump
    let manifest_path = dir.path().join("Cargo.toml");
    let args = BumpArgs {
        manifest_path: Some(manifest_path),
        minor: true,
        version: None,
        auto: false,
        major: false,
        patch: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify the bump commit
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    let tree = commit.tree().expect("Failed to get tree");

    // Verify ALL files are still present
    assert!(
        tree.lookup_entry_by_path("Cargo.toml")
            .expect("Failed to lookup")
            .is_some(),
        "Cargo.toml should be in commit"
    );
    assert!(
        tree.lookup_entry_by_path("README.md")
            .expect("Failed to lookup")
            .is_some(),
        "README.md should still be in commit (not deleted!)"
    );
    assert!(
        tree.lookup_entry_by_path(".gitignore")
            .expect("Failed to lookup")
            .is_some(),
        ".gitignore should still be in commit (not deleted!)"
    );
    assert!(
        tree.lookup_entry_by_path("src/lib.rs")
            .expect("Failed to lookup")
            .is_some(),
        "src/lib.rs should still be in commit (not deleted!)"
    );
    assert!(
        tree.lookup_entry_by_path("docs/guide.md")
            .expect("Failed to lookup")
            .is_some(),
        "docs/guide.md should still be in commit (not deleted!)"
    );

    // Verify Cargo.toml version was updated
    let cargo_entry = tree
        .lookup_entry_by_path("Cargo.toml")
        .expect("Failed to lookup")
        .expect("Cargo.toml not in tree");

    let cargo_blob = cargo_entry
        .object()
        .expect("Failed to get blob")
        .try_into_blob()
        .expect("Not a blob");

    let cargo_content = cargo_blob.data.to_str_lossy();
    assert!(
        cargo_content.contains("version = \"0.6.0\""),
        "Cargo.toml version should be bumped (minor: 0.5.0 -> 0.6.0)"
    );
}

/// Test that the git index is reset after a bump commit.
///
/// This test verifies that after the bump command creates a commit, the git
/// index matches HEAD and there are no spurious staged changes. This is a
/// regression test for a bug where the index was left with stale staged
/// changes after bump because the commit was created via direct tree
/// manipulation bypassing the index.
#[test]
#[serial_test::serial]
fn test_bump_resets_index_after_commit() {
    let cargo_content = r#"
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#;

    // Create a test cargo project
    let dir = create_temp_cargo_project(cargo_content);
    let manifest_path = dir.path().join("Cargo.toml");

    // Initialize git repo and create initial commit
    init_test_git_repo(dir.path());
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create and stage an unrelated file change to simulate pre-existing staged
    // changes This is the scenario that caused the bug: someone had staged
    // changes, then ran bump, and the index was left in a confused state
    let readme_path = dir.path().join("README.md");
    std::fs::write(&readme_path, "# Test\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Perform the bump
    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        version: None,
        auto: false,
        major: false,
        minor: false,
        patch: true,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,
        no_readme: true,
    };
    bump(args).expect("Bump should succeed");

    // Verify there are no staged changes (index matches HEAD)
    // This is the key assertion - previously the index would have stale staged
    // changes
    let diff_index_output = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to run git diff --cached");

    assert!(
        diff_index_output.status.success(),
        "Index should match HEAD (no staged changes)"
    );

    // Also check there are no modified (unstaged) tracked files
    let diff_output = std::process::Command::new("git")
        .args(["diff", "--quiet"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to run git diff");

    assert!(
        diff_output.status.success(),
        "Working tree should match HEAD (no unstaged changes to tracked files)"
    );
}

/// Test that README.md uses selective staging for version changes.
///
/// This test verifies that when README.md has both version-related changes
/// (e.g., `my-crate = "0.1.0"` -> `"0.2.0"`) and non-version changes (e.g.,
/// documentation updates), only the version-related changes are committed.
#[test]
#[serial_test::serial]
fn test_readme_selective_staging() {
    let dir = tempfile::tempdir().unwrap();

    // Create initial Cargo.toml and README.md
    let initial_cargo_toml = r#"[package]
name = "my-test-crate"
version = "0.1.0"
edition = "2021"
"#;

    let initial_readme = r#"# My Test Crate

Add to your Cargo.toml:

```toml
my-test-crate = "0.1.0"
```

## Description

This is the original description.
"#;

    let manifest_path = dir.path().join("Cargo.toml");
    std::fs::write(&manifest_path, initial_cargo_toml).unwrap();

    let readme_path = dir.path().join("README.md");
    std::fs::write(&readme_path, initial_readme).unwrap();

    // Create src/lib.rs for valid cargo project
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    // Initialize git repo
    init_test_git_repo(dir.path());
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Modify README.md with BOTH version change and non-version change
    let modified_readme = r#"# My Test Crate

Add to your Cargo.toml:

```toml
my-test-crate = "0.1.0"
```

## Description

This is the UPDATED description with more details.

## New Section

This is a new section that was added.
"#;
    std::fs::write(&readme_path, modified_readme).unwrap();

    // Run bump - this should only commit the version change in README
    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,    // Skip Cargo.lock for this test
        no_readme: false, // DO update README
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify the commit
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    let tree = commit.tree().expect("Failed to get tree");

    // Verify README.md in commit
    let readme_entry = tree
        .lookup_entry_by_path("README.md")
        .expect("Failed to lookup README")
        .expect("README not in commit");

    let blob = readme_entry
        .object()
        .expect("Failed to get blob")
        .try_into_blob()
        .expect("Not a blob");

    let committed_readme = blob.data.to_str_lossy();

    // The commit should have the VERSION change (0.1.0 -> 0.1.1 for patch bump)
    assert!(
        committed_readme.contains(r#"my-test-crate = "0.1.1""#),
        "README in commit should have updated version (0.1.1)"
    );

    // The commit should NOT have the description change
    assert!(
        committed_readme.contains("This is the original description."),
        "README in commit should have ORIGINAL description, not updated"
    );
    assert!(
        !committed_readme.contains("UPDATED description"),
        "README in commit should NOT have the updated description"
    );
    assert!(
        !committed_readme.contains("## New Section"),
        "README in commit should NOT have the new section"
    );

    // Verify working directory still has ALL changes
    let working_readme = std::fs::read_to_string(&readme_path).expect("Failed to read README");
    assert!(
        working_readme.contains("UPDATED description"),
        "Working README should still have the updated description"
    );
    assert!(
        working_readme.contains("## New Section"),
        "Working README should still have the new section"
    );
}

/// Test that Cargo.lock uses selective staging for version changes.
///
/// This test verifies that when Cargo.lock has both our crate's version
/// change and other dependency updates (pre-existing uncommitted changes),
/// only our crate's version change is committed.
#[test]
#[serial_test::serial]
fn test_cargo_lock_selective_staging() {
    let dir = tempfile::tempdir().unwrap();

    // Create initial Cargo.toml
    let initial_cargo_toml = r#"[package]
name = "my-test-crate"
version = "0.1.0"
edition = "2021"

[dependencies]
# No real dependencies - we'll simulate Cargo.lock content
"#;

    // Create initial Cargo.lock (simulating a lock file with our crate and others)
    let initial_cargo_lock = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "my-test-crate"
version = "0.1.0"

[[package]]
name = "other-dependency"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    let manifest_path = dir.path().join("Cargo.toml");
    std::fs::write(&manifest_path, initial_cargo_toml).unwrap();

    let cargo_lock_path = dir.path().join("Cargo.lock");
    std::fs::write(&cargo_lock_path, initial_cargo_lock).unwrap();

    // Create src/lib.rs for valid cargo project
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    // Initialize git repo
    init_test_git_repo(dir.path());
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Simulate a pre-existing dependency update in Cargo.lock
    // (like someone ran `cargo update` but didn't commit)
    let modified_cargo_lock = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "my-test-crate"
version = "0.1.0"

[[package]]
name = "other-dependency"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
    std::fs::write(&cargo_lock_path, modified_cargo_lock).unwrap();

    // Run bump with --no-lock to skip cargo update (we're manually controlling
    // Cargo.lock) But we need the selective staging logic to run, so we'll use
    // a custom approach First update Cargo.toml version
    let updated_cargo_toml = r#"[package]
name = "my-test-crate"
version = "0.2.0"
edition = "2021"

[dependencies]
# No real dependencies - we'll simulate Cargo.lock content
"#;
    std::fs::write(&manifest_path, updated_cargo_toml).unwrap();

    // Now update Cargo.lock to have our new version AND the dependency update
    let final_cargo_lock = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "my-test-crate"
version = "0.2.0"

[[package]]
name = "other-dependency"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
    std::fs::write(&cargo_lock_path, final_cargo_lock).unwrap();

    // Use commit function directly to test selective staging
    use super::commit::{
        AdditionalFile,
        FileType,
        commit_version_changes_with_files,
    };

    // Get HEAD content of Cargo.lock (the initial version)
    let head_cargo_lock = initial_cargo_lock;

    let additional_files = vec![AdditionalFile {
        path: cargo_lock_path.clone(),
        working_content: final_cargo_lock.to_string(),
        head_content: Some(head_cargo_lock.to_string()),
        file_type: FileType::CargoLock,
    }];

    let result = commit_version_changes_with_files(
        &manifest_path,
        "my-test-crate",
        "0.1.0",
        "0.2.0",
        &additional_files,
    );
    assert!(result.is_ok(), "Commit failed: {:?}", result.err());

    // Verify the commit
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    let tree = commit.tree().expect("Failed to get tree");

    // Verify Cargo.lock in commit
    let lock_entry = tree
        .lookup_entry_by_path("Cargo.lock")
        .expect("Failed to lookup Cargo.lock")
        .expect("Cargo.lock not in commit");

    let blob = lock_entry
        .object()
        .expect("Failed to get blob")
        .try_into_blob()
        .expect("Not a blob");

    let committed_lock = blob.data.to_str_lossy();

    // The commit should have OUR crate's version change
    assert!(
        committed_lock.contains(r#"name = "my-test-crate""#),
        "Cargo.lock should have our crate"
    );
    assert!(
        committed_lock.contains(r#"version = "0.2.0""#),
        "Cargo.lock should have our crate's new version"
    );

    // The commit should NOT have the other-dependency update
    assert!(
        committed_lock.contains(r#"name = "other-dependency""#),
        "Cargo.lock should have other-dependency"
    );
    assert!(
        committed_lock.contains(r#"version = "1.0.0""#),
        "Cargo.lock should have other-dependency's ORIGINAL version (1.0.0), not 2.0.0"
    );
    assert!(
        !committed_lock.matches(r#"version = "2.0.0""#).any(|_| true),
        "Cargo.lock should NOT have the updated other-dependency version"
    );

    // Verify working directory still has the dependency update
    let working_lock =
        std::fs::read_to_string(&cargo_lock_path).expect("Failed to read Cargo.lock");
    assert!(
        working_lock.contains(r#"version = "2.0.0""#),
        "Working Cargo.lock should still have the dependency update"
    );
}

#[test]
#[serial_test::serial]
fn test_cargo_lock_selective_staging_covers_workspace_members() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("Cargo.toml");
    let member_dir = dir.path().join("member");
    let cargo_lock_path = dir.path().join("Cargo.lock");
    std::fs::create_dir_all(member_dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "// workspace root\n").unwrap();
    std::fs::write(member_dir.join("src/lib.rs"), "// workspace member\n").unwrap();

    let initial_manifest = r#"[package]
name = "workspace-root"
version = "0.20.1"
edition = "2024"

[workspace]
members = ["member"]
resolver = "2"

[workspace.package]
version = "0.20.1"
edition = "2024"
"#;
    std::fs::write(&manifest_path, initial_manifest).unwrap();
    std::fs::write(
        member_dir.join("Cargo.toml"),
        r#"[package]
name = "workspace-member"
version.workspace = true
edition.workspace = true
"#,
    )
    .unwrap();

    let initial_lock = r#"version = 4

[[package]]
name = "registry-dependency"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "workspace-member"
version = "0.20.1"

[[package]]
name = "workspace-root"
version = "0.20.1"
"#;
    std::fs::write(&cargo_lock_path, initial_lock).unwrap();

    init_test_git_repo(dir.path());
    std::process::Command::new("git")
        .args([
            "add",
            "Cargo.lock",
            "member/Cargo.toml",
            "member/src/lib.rs",
            "src/lib.rs",
        ])
        .current_dir(dir.path())
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "test: add workspace"])
        .current_dir(dir.path())
        .status()
        .unwrap();

    std::fs::write(&manifest_path, initial_manifest.replace("0.20.1", "0.20.2")).unwrap();
    let bumped_lock = initial_lock
        .replace(
            "registry-dependency\"\nversion = \"1.0.0",
            "registry-dependency\"\nversion = \"2.0.0",
        )
        .replace("0.20.1", "0.20.2");
    std::fs::write(&cargo_lock_path, &bumped_lock).unwrap();

    use super::commit::{
        AdditionalFile,
        FileType,
        commit_version_changes_with_files,
    };
    commit_version_changes_with_files(
        &manifest_path,
        "workspace-root",
        "0.20.1",
        "0.20.2",
        &[AdditionalFile {
            path: cargo_lock_path.clone(),
            working_content: bumped_lock,
            head_content: Some(initial_lock.to_string()),
            file_type: FileType::CargoLock,
        }],
    )
    .unwrap();

    let committed_lock = std::process::Command::new("git")
        .args(["show", "HEAD:Cargo.lock"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let committed_lock = String::from_utf8(committed_lock.stdout).unwrap();
    assert!(committed_lock.contains("name = \"workspace-root\"\nversion = \"0.20.2\""));
    assert!(committed_lock.contains("name = \"workspace-member\"\nversion = \"0.20.2\""));
    assert!(committed_lock.contains("name = \"registry-dependency\"\nversion = \"1.0.0\""));

    let working_lock = std::fs::read_to_string(cargo_lock_path).unwrap();
    assert!(working_lock.contains("name = \"registry-dependency\"\nversion = \"2.0.0\""));
}

/// Test that all files (Cargo.toml, README.md, Cargo.lock) use selective
/// staging when they have non-version changes.
#[test]
#[serial_test::serial]
fn test_all_files_selective_staging() {
    let dir = tempfile::tempdir().unwrap();

    // Create initial files
    let initial_cargo_toml = r#"[package]
name = "test-crate"
version = "1.0.0"
description = "Original description"
edition = "2021"
"#;

    let initial_readme = r#"# Test Crate

```toml
test-crate = "1.0.0"
```

Original readme content.
"#;

    let initial_cargo_lock = r#"# This file is automatically @generated by Cargo.
version = 3

[[package]]
name = "test-crate"
version = "1.0.0"

[[package]]
name = "dep"
version = "1.0.0"
"#;

    let manifest_path = dir.path().join("Cargo.toml");
    std::fs::write(&manifest_path, initial_cargo_toml).unwrap();

    let readme_path = dir.path().join("README.md");
    std::fs::write(&readme_path, initial_readme).unwrap();

    let cargo_lock_path = dir.path().join("Cargo.lock");
    std::fs::write(&cargo_lock_path, initial_cargo_lock).unwrap();

    // Create src/lib.rs
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test\n").unwrap();

    // Initialize git repo
    init_test_git_repo(dir.path());
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Modify ALL files with version AND non-version changes
    let modified_cargo_toml = r#"[package]
name = "test-crate"
version = "1.0.0"
description = "UPDATED description"
edition = "2021"
"#;
    std::fs::write(&manifest_path, modified_cargo_toml).unwrap();

    let modified_readme = r#"# Test Crate

```toml
test-crate = "1.0.0"
```

UPDATED readme content with new docs.
"#;
    std::fs::write(&readme_path, modified_readme).unwrap();

    let modified_cargo_lock = r#"# This file is automatically @generated by Cargo.
version = 3

[[package]]
name = "test-crate"
version = "1.0.0"

[[package]]
name = "dep"
version = "2.0.0"
"#;
    std::fs::write(&cargo_lock_path, modified_cargo_lock).unwrap();

    // Run bump
    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,    // Don't run cargo update
        no_readme: false, // Do update README
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify the commit
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    let tree = commit.tree().expect("Failed to get tree");

    // Check Cargo.toml - only version change should be committed
    let cargo_entry = tree
        .lookup_entry_by_path("Cargo.toml")
        .expect("Failed to lookup")
        .expect("Cargo.toml not found");
    let blob = cargo_entry.object().unwrap().try_into_blob().unwrap();
    let committed_cargo = blob.data.to_str_lossy();

    assert!(
        committed_cargo.contains(r#"version = "1.0.1""#),
        "Cargo.toml should have new version"
    );
    assert!(
        committed_cargo.contains(r#"description = "Original description""#),
        "Cargo.toml should have ORIGINAL description"
    );

    // Check README.md - only version line should be committed
    let readme_entry = tree
        .lookup_entry_by_path("README.md")
        .expect("Failed to lookup")
        .expect("README.md not found");
    let blob = readme_entry.object().unwrap().try_into_blob().unwrap();
    let committed_readme = blob.data.to_str_lossy();

    assert!(
        committed_readme.contains(r#"test-crate = "1.0.1""#),
        "README should have new version"
    );
    assert!(
        committed_readme.contains("Original readme content."),
        "README should have ORIGINAL content"
    );

    // Verify working directory still has ALL changes
    let working_cargo = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(working_cargo.contains("UPDATED description"));

    let working_readme = std::fs::read_to_string(&readme_path).unwrap();
    assert!(working_readme.contains("UPDATED readme content"));
}

// ============================================================================
// Hook Integration Tests (Unix only - hooks use shell commands)
// ============================================================================

/// Test that pre_bump_hooks are executed with correct version substitution.
#[test]
#[serial_test::serial]
#[cfg(unix)]
fn test_pre_bump_hooks_executed() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("Cargo.toml");
    let marker_file = dir.path().join("pre_bump_marker.txt");

    // Create Cargo.toml with pre_bump_hooks configuration
    let cargo_content = format!(
        r#"[package]
name = "test-hooks"
version = "1.0.0"

[package.metadata.version-info]
pre_bump_hooks = ["echo '{{{{version}}}}' > {}"]
"#,
        marker_file.display()
    );
    std::fs::write(&manifest_path, &cargo_content).unwrap();

    // Create src directory
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    init_test_git_repo(dir.path());

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true, // Skip commit to isolate hook test
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify hook was executed with correct version
    assert!(
        marker_file.exists(),
        "Pre-bump hook should have created marker file"
    );
    let content = std::fs::read_to_string(&marker_file).unwrap();
    assert_eq!(
        content.trim(),
        "1.0.1",
        "Hook should receive the NEW version"
    );
}

/// Test that failing pre_bump_hooks abort the bump operation.
#[test]
#[serial_test::serial]
#[cfg(unix)]
fn test_pre_bump_hooks_failure_aborts_bump() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("Cargo.toml");

    // Create Cargo.toml with a failing pre_bump_hook
    let cargo_content = r#"[package]
name = "test-hooks"
version = "1.0.0"

[package.metadata.version-info]
pre_bump_hooks = ["exit 1"]
"#;
    std::fs::write(&manifest_path, cargo_content).unwrap();

    // Create src directory
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    init_test_git_repo(dir.path());

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_err(), "Bump should fail when pre_bump_hook fails");

    // Verify error message mentions hook failure
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Hook failed") || err_msg.contains("exit code"),
        "Error should mention hook failure: {}",
        err_msg
    );
}

/// Test that post_bump_hooks are executed after successful commit.
#[test]
#[serial_test::serial]
#[cfg(unix)]
fn test_post_bump_hooks_executed_after_commit() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("Cargo.toml");
    let marker_file = dir.path().join("post_bump_marker.txt");

    // Create Cargo.toml with post_bump_hooks configuration
    let cargo_content = format!(
        r#"[package]
name = "test-hooks"
version = "1.0.0"

[package.metadata.version-info]
post_bump_hooks = ["echo '{{{{version}}}}' > {}"]
"#,
        marker_file.display()
    );
    std::fs::write(&manifest_path, &cargo_content).unwrap();

    // Create src directory
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    init_test_git_repo(dir.path());

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false, // Need commit for post_bump_hooks
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify post-bump hook was executed
    assert!(
        marker_file.exists(),
        "Post-bump hook should have created marker file"
    );
    let content = std::fs::read_to_string(&marker_file).unwrap();
    assert_eq!(
        content.trim(),
        "1.0.1",
        "Hook should receive the NEW version"
    );
}

/// Test that post_bump_hooks are NOT executed when --no-commit is used.
#[test]
#[serial_test::serial]
#[cfg(unix)]
fn test_post_bump_hooks_skipped_with_no_commit() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("Cargo.toml");
    let marker_file = dir.path().join("post_bump_marker.txt");

    // Create Cargo.toml with post_bump_hooks configuration
    let cargo_content = format!(
        r#"[package]
name = "test-hooks"
version = "1.0.0"

[package.metadata.version-info]
post_bump_hooks = ["echo 'executed' > {}"]
"#,
        marker_file.display()
    );
    std::fs::write(&manifest_path, &cargo_content).unwrap();

    // Create src directory
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    init_test_git_repo(dir.path());

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true, // Skip commit
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify post-bump hook was NOT executed
    assert!(
        !marker_file.exists(),
        "Post-bump hook should NOT run when --no-commit is used"
    );
}

/// Test that additional_files are included in the version bump commit.
#[test]
#[serial_test::serial]
#[cfg(unix)]
fn test_additional_files_included_in_commit() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("Cargo.toml");
    let package_json = dir.path().join("package.json");

    // Create initial package.json
    std::fs::write(&package_json, r#"{"version": "1.0.0"}"#).unwrap();

    // Create Cargo.toml with hooks that update package.json
    let cargo_content = format!(
        r#"[package]
name = "test-hooks"
version = "1.0.0"

[package.metadata.version-info]
pre_bump_hooks = ["echo '{{\"version\": \"{{{{version}}}}\"}}' > {}"]
additional_files = ["package.json"]
"#,
        package_json.display()
    );
    std::fs::write(&manifest_path, &cargo_content).unwrap();

    // Create src directory
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    // Initialize git and add package.json to initial commit
    std::process::Command::new("git")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "Cargo.toml", "package.json", "src/lib.rs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: false,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify package.json is in the commit with updated version
    let repo = gix::open(dir.path()).expect("Failed to open repo");
    let head = repo.head().expect("Failed to read HEAD");
    let commit_id = head.id().expect("HEAD not pointing to commit");
    let commit = repo
        .find_object(commit_id)
        .expect("Failed to find commit")
        .try_into_commit()
        .expect("Not a commit");

    let tree = commit.tree().expect("Failed to get tree");

    let entry = tree
        .lookup_entry_by_path("package.json")
        .expect("Failed to lookup")
        .expect("package.json not in commit");

    let blob = entry.object().unwrap().try_into_blob().unwrap();
    let content = blob.data.to_str_lossy();

    assert!(
        content.contains("1.0.1"),
        "package.json should have new version in commit: {}",
        content
    );
}

/// Test that multiple pre_bump_hooks run in order.
#[test]
#[serial_test::serial]
#[cfg(unix)]
fn test_multiple_pre_bump_hooks_run_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("Cargo.toml");
    let marker_file = dir.path().join("hook_order.txt");

    // Create Cargo.toml with multiple hooks that append to a file
    let cargo_content = format!(
        r#"[package]
name = "test-hooks"
version = "1.0.0"

[package.metadata.version-info]
pre_bump_hooks = [
    "echo 'first' >> {}",
    "echo 'second' >> {}",
    "echo 'third' >> {}"
]
"#,
        marker_file.display(),
        marker_file.display(),
        marker_file.display()
    );
    std::fs::write(&manifest_path, &cargo_content).unwrap();

    // Create src directory
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "// Test library\n").unwrap();

    init_test_git_repo(dir.path());

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump failed: {:?}", result.err());

    // Verify hooks ran in order
    let content = std::fs::read_to_string(&marker_file).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines, vec!["first", "second", "third"]);
}

/// Test that empty hook configuration works (no hooks configured).
#[test]
#[serial_test::serial]
fn test_no_hooks_configured() {
    let dir = create_temp_cargo_project(
        r#"[package]
name = "test-no-hooks"
version = "1.0.0"
"#,
    );
    let manifest_path = dir.path().join("Cargo.toml");

    init_test_git_repo(dir.path());

    let args = BumpArgs {
        manifest_path: Some(manifest_path.clone()),
        patch: true,
        version: None,
        auto: false,
        major: false,
        minor: false,
        owner: None,
        repo: None,
        github_token: None,
        no_commit: true,
        no_lock: true,
        no_readme: true,
    };

    let result = bump(args);
    assert!(result.is_ok(), "Bump should work without hooks configured");

    let content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(content.contains("version = \"1.0.1\""));
}
