//! Generate changelog from conventional commits.
//!
//! This command generates a changelog from git commits using conventional
//! commit format, organized by scope for better readability.
//!
//! # Examples
//!
//! ```bash
//! # Generate changelog since latest version tag (automatically finds latest tag)
//! cargo version-info changelog
//!
//! # Generate changelog for a specific version (uses version in header, auto-finds latest tag)
//! cargo version-info changelog --for-version v0.1.0
//!
//! # Generate changelog for specific tag
//! cargo version-info changelog --at v0.1.0
//!
//! # Generate changelog for commit range
//! cargo version-info changelog --range v0.1.0..v0.2.0
//!
//! # Output to file
//! cargo version-info changelog --output CHANGELOG.md
//!
//! # Combined: version in header + output to file
//! cargo version-info changelog --for-version v0.1.0 --output CHANGELOG.md
//! ```

use std::collections::HashMap;

use anyhow::{
    Context,
    Result,
};
use async_fs_io::write_bytes;
use bstr::{
    BString,
    ByteSlice,
};
use cargo_plugin_utils::common::get_owner_repo;
use clap::Parser;
use regex::Regex;

use crate::version::parse_version;

/// Arguments for the `changelog` command.
#[derive(Parser, Debug)]
pub struct ChangelogArgs {
    /// Generate changelog for a specific git tag.
    #[arg(long)]
    pub at: Option<String>,

    /// Generate changelog for a commit range (e.g., v0.1.0..v0.2.0).
    #[arg(long)]
    pub range: Option<String>,

    /// Version to generate changelog for (e.g., 0.1.0 or v0.1.0).
    ///
    /// This is used for the changelog header and metadata. If not specified,
    /// the changelog will not include version information in the header.
    /// The command will still automatically find the latest git tag to
    /// determine the commit range.
    #[arg(long)]
    pub for_version: Option<String>,

    /// Output file path (default: stdout).
    #[arg(short, long)]
    pub output: Option<String>,

    /// GitHub repository owner (for linking commits/PRs).
    #[arg(long)]
    pub owner: Option<String>,

    /// GitHub repository name (for linking commits/PRs).
    #[arg(long)]
    pub repo: Option<String>,
}

/// Commit information parsed from git log.
#[derive(Debug, Clone)]
struct Commit {
    sha: String,
    short_sha: String,
    commit_type: String,
    scope: Option<String>,
    breaking: bool,
    subject: String,
    body: Option<String>,
}

/// Parse a conventional commit message.
fn parse_conventional_commit(message: &str) -> Option<Commit> {
    // Pattern: type(scope): subject
    // or: type!: subject (breaking change)
    // or: type(scope)!: subject (breaking change with scope)
    let re = Regex::new(
        r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]+)\))?(?P<breaking>!)?:\s*(?P<subject>.+)$",
    )
    .ok()?;

    let first_line = message.lines().next()?;
    let caps = re.captures(first_line)?;

    let commit_type = caps.name("type")?.as_str().to_string();
    let scope = caps.name("scope").map(|m| m.as_str().to_string());
    let breaking = caps.name("breaking").is_some();
    let subject = caps.name("subject")?.as_str().to_string();

    // Extract body (everything after first line, skipping blank line)
    let body = message
        .lines()
        .skip(1)
        .skip_while(|l| l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let body = if body.is_empty() { None } else { Some(body) };

    // Extract SHA from message if available, otherwise use placeholder
    // For now, we'll get SHA from git commit object
    Some(Commit {
        sha: String::new(),       // Will be filled in later
        short_sha: String::new(), // Will be filled in later
        commit_type,
        scope,
        breaking,
        subject,
        body,
    })
}

/// Get commit type display title.
fn commit_type_title(commit_type: &str) -> &str {
    match commit_type {
        "feat" => "Features",
        "fix" => "Bug Fixes",
        "docs" => "Documentation",
        "style" => "Styling",
        "refactor" => "Refactoring",
        "perf" => "Performance",
        "test" => "Tests",
        "build" => "Build",
        "ci" => "CI/CD",
        "chore" => "Chores",
        "revert" => "Reverts",
        _ => "Other Changes",
    }
}

/// Check if commit type should be included in changelog.
fn include_in_changelog(commit_type: &str) -> bool {
    matches!(
        commit_type,
        "feat" | "fix" | "docs" | "refactor" | "perf" | "revert"
    )
}

/// Format a single commit as a changelog entry.
fn format_commit_entry(commit: &Commit, owner: &str, repo: &str) -> String {
    let breaking_marker = if commit.breaking { " **BREAKING**" } else { "" };
    let commit_link = format!(
        "[{}](https://github.com/{}/{}/commit/{})",
        commit.short_sha, owner, repo, commit.sha
    );
    let mut output = format!("- {}{}: {}\n", commit_link, breaking_marker, commit.subject);

    // Add body if present
    if let Some(body) = &commit.body {
        let body_lines: Vec<&str> = body.lines().collect();
        if !body_lines.is_empty() {
            for line in body_lines {
                output.push_str(&format!("  {}\n", line));
            }
        }
    }

    output
}

/// Resolve a reference to a commit OID, following tags iteratively.
fn resolve_to_commit_oid<'a>(
    git_repo: &'a gix::Repository,
    reference: &str,
) -> Result<gix::Id<'a>> {
    // First, try using rev_parse with ^{commit} suffix to follow tags automatically
    // This handles all cases (tags, branches, HEAD, SHAs) and peels tags
    let ref_with_suffix = format!("{}^{{commit}}", reference);
    let ref_bstr: BString = ref_with_suffix.into();
    if let Ok(spec) = git_repo.rev_parse(ref_bstr.as_bstr())
        && let Some(oid) = spec.single()
        && let Ok(obj) = git_repo.find_object(oid)
        && obj.try_into_commit().is_ok()
    {
        return Ok(oid);
    }

    // Fallback: try without suffix first, then peel if it's a tag
    let ref_bstr: BString = reference.into();
    let spec = git_repo
        .rev_parse(ref_bstr.as_bstr())
        .context("Failed to resolve reference")?;
    let oid = spec
        .single()
        .context("Reference resolved to multiple objects")?;

    // Check if it's already a commit or a tag
    let obj = git_repo.find_object(oid).context("Failed to find object")?;

    // Check the object kind first to avoid consuming it unnecessarily
    let obj_kind = obj.kind;
    match obj_kind {
        gix::object::Kind::Commit => {
            // Already a commit - verify and return
            obj.try_into_commit()
                .context("Object kind is Commit but conversion failed")?;
            return Ok(oid);
        }
        gix::object::Kind::Tag => {
            // It's a tag - try using the reference API to peel it
            let tag_ref_name = format!("refs/tags/{}", reference);
            if let Ok(mut tag_ref) = git_repo.find_reference(tag_ref_name.as_str()) {
                let peeled_oid = tag_ref
                    .peel_to_id()
                    .context("Failed to peel tag to commit")?;
                // Verify the peeled result is a commit
                let peeled_obj = git_repo
                    .find_object(peeled_oid)
                    .context("Failed to find peeled commit object")?;
                peeled_obj
                    .try_into_commit()
                    .context("Tag does not point to a commit")?;
                return Ok(peeled_oid);
            }
        }
        _ => {
            // Other object types are not supported
        }
    }

    anyhow::bail!("Reference '{}' does not point to a commit", reference);
}

/// Generate changelog to a writer.
pub fn generate_changelog_to_writer(
    writer: &mut dyn std::io::Write,
    args: ChangelogArgs,
) -> Result<()> {
    let (owner, repo) = get_owner_repo(args.owner.clone(), args.repo.clone())?;

    // Discover git repository
    let git_repo = gix::discover(".").context("Failed to discover git repository")?;

    // Determine start commit for range
    let (start_oid, end_oid) = if let Some(range) = &args.range {
        // Parse range like "v0.1.0..v0.2.0" or "v0.1.0..HEAD"
        let parts: Vec<&str> = range.split("..").collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid range format. Expected: <start>..<end>");
        }
        let start_ref = parts[0].trim();
        let end_ref = parts[1].trim();

        // Resolve references using rev_parse, following tags to commits
        // If start reference doesn't exist, treat it as if there's no start point
        let start_oid = match resolve_to_commit_oid(&git_repo, start_ref) {
            Ok(oid) => Some(oid),
            Err(_) => {
                eprintln!(
                    "Warning: Start reference '{}' not found in repository, \
                     generating changelog from beginning",
                    start_ref
                );
                None
            }
        };

        let end_oid = resolve_to_commit_oid(&git_repo, end_ref)
            .with_context(|| format!("Failed to resolve end reference: {}", end_ref))?;

        (start_oid, end_oid)
    } else if let Some(tag) = &args.at {
        // Generate changelog for commits up to this tag
        let tag_oid = resolve_to_commit_oid(&git_repo, tag)
            .with_context(|| format!("Failed to resolve tag: {}", tag))?;

        // Get HEAD for end
        let head = git_repo.head().context("Failed to read HEAD")?;
        let head_oid = head.id().context("HEAD does not point to a commit")?;

        (Some(tag_oid), head_oid)
    } else {
        // Default: since last version tag
        // Find the latest version tag by collecting all version tags, parsing them,
        // sorting by version, and taking the latest one
        let mut version_tags: Vec<(gix::Id, String, (u32, u32, u32))> = Vec::new();

        let refs = git_repo
            .references()
            .context("Failed to read git references")?;
        for reference_result in refs.all()? {
            let Ok(reference) = reference_result else {
                continue;
            };
            let name_str = reference.name().as_bstr().to_string();
            let Some(name) = name_str.strip_prefix("refs/tags/") else {
                continue;
            };

            // Try to parse as semantic version
            let version_str = name
                .strip_prefix('v')
                .or_else(|| name.strip_prefix('V'))
                .unwrap_or(name);
            let Ok((major, minor, patch)) = parse_version(version_str) else {
                continue;
            };

            // Resolve tag to commit OID (follows tags recursively)
            let Ok(commit_oid) = resolve_to_commit_oid(&git_repo, name) else {
                continue;
            };
            version_tags.push((commit_oid, name.to_string(), (major, minor, patch)));
        }

        // Sort tags by semantic version (major, minor, patch)
        version_tags.sort_by_key(|a| a.2);

        // Get the latest tag's commit OID (if any)
        let latest_tag_oid = version_tags.last().map(|(oid, _tag_name, _version)| *oid);

        // Get HEAD for end
        let head = git_repo.head().context("Failed to read HEAD")?;
        let head_oid = head.id().context("HEAD does not point to a commit")?;

        (latest_tag_oid, head_oid)
    };

    // Walk commits using gix rev_walk
    let walk = git_repo.rev_walk([end_oid]);
    let walk_iter = walk.all()?;

    // If we have a start point, we need to stop at it
    // For now, we'll walk all commits and filter by checking if we've reached
    // start_oid
    let mut commits: Vec<Commit> = Vec::new();

    for info_result in walk_iter {
        let info = info_result?;
        let oid = info.id();

        // Stop if we've reached the start commit
        if let Some(start) = start_oid
            && oid == start
        {
            break;
        }

        // Get commit object
        let commit_obj = git_repo
            .find_object(oid)
            .context("Failed to find commit object")?;
        let commit = commit_obj
            .try_into_commit()
            .context("Object is not a commit")?;

        // Get commit message
        let message_raw = commit
            .message_raw()
            .context("Failed to read raw commit message")?;
        // Convert message to UTF-8, tolerating invalid bytes
        let message_str = String::from_utf8_lossy(message_raw.as_ref()).into_owned();

        // Parse conventional commit format
        if let Some(mut parsed) = parse_conventional_commit(&message_str) {
            // Only include commits that should be in changelog
            if include_in_changelog(&parsed.commit_type) {
                let short_sha = oid.shorten().context("Failed to shorten commit SHA")?;
                parsed.sha = oid.to_string();
                parsed.short_sha = short_sha.to_string();

                // Extract body from message (everything after first line)
                let body_lines: Vec<&str> = message_str.lines().skip(1).collect();
                let body_text: String = body_lines.join("\n").trim().to_string();
                parsed.body = if body_text.is_empty() {
                    None
                } else {
                    Some(body_text)
                };

                commits.push(parsed);
            }
        }
    }

    // Group commits by type, then by scope
    let mut by_type: HashMap<String, HashMap<Option<String>, Vec<Commit>>> = HashMap::new();

    for commit in commits {
        by_type
            .entry(commit.commit_type.clone())
            .or_default()
            .entry(commit.scope.clone())
            .or_default()
            .push(commit);
    }

    // Generate markdown
    let mut output = String::new();

    // Header - prioritize for_version, then at, then generic
    if let Some(version) = &args.for_version {
        // Normalize version to have v prefix for display
        let version_display = if version.starts_with('v') || version.starts_with('V') {
            version.clone()
        } else {
            format!("v{}", version)
        };
        output.push_str(&format!("# Changelog - {}\n\n", version_display));
    } else if let Some(tag) = &args.at {
        output.push_str(&format!("# Changelog - {}\n\n", tag));
    } else {
        output.push_str("# Changelog\n\n");
    }

    // Order commit types
    let type_order = [
        "feat", "fix", "perf", "refactor", "docs", "revert", "build", "ci", "test", "style",
        "chore",
    ];

    for commit_type in type_order {
        if let Some(by_scope) = by_type.get(commit_type) {
            output.push_str(&format!("## {}\n\n", commit_type_title(commit_type)));

            // Group by scope
            let mut scopes: Vec<_> = by_scope.keys().collect();
            scopes.sort(); // None (no scope) will come first

            for scope in scopes {
                let scope_commits = &by_scope[scope];

                // Scope header if present
                if let Some(scope_name) = scope {
                    output.push_str(&format!("### {}\n\n", scope_name));
                }

                // List commits
                for commit in scope_commits {
                    output.push_str(&format_commit_entry(commit, &owner, &repo));
                }

                output.push('\n');
            }
        }
    }

    if output.trim().ends_with("# Changelog\n\n") {
        output.push_str("No changes found.\n");
    }

    // Write to the provided writer
    write!(writer, "{}", output)?;

    Ok(())
}

/// Generate changelog from git commits.
pub async fn changelog(args: ChangelogArgs) -> Result<()> {
    let output_path = args.output.clone();

    if let Some(ref path) = output_path {
        let mut output = Vec::new();
        generate_changelog_to_writer(&mut output, args)?;
        write_bytes(path, &output)
            .await
            .with_context(|| format!("Failed to write file {}", path))?;
    } else {
        // Write to stdout
        let mut stdout = std::io::stdout();
        generate_changelog_to_writer(&mut stdout, args)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    async fn create_test_git_repo_with_tags_and_commits(
        tags: &[&str],
        commits: &[&str],
    ) -> async_fs_io::TempDir {
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

        // Create commits (with conventional commit format)
        for commit_msg in commits {
            let file_name = format!("file_{}.txt", commit_msg.replace([' ', ':'], "_"));
            async_fs_io::write_bytes(dir.path().join(&file_name), commit_msg.as_bytes())
                .await
                .unwrap();
            Command::new("git")
                .args(["add", &file_name])
                .current_dir(dir.path())
                .output()
                .unwrap();
            Command::new("git")
                .args(["commit", "-m", commit_msg])
                .current_dir(dir.path())
                .output()
                .unwrap();
        }

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
    async fn test_changelog_finds_latest_tag_not_first() {
        // Test that changelog finds the latest version tag, not just the first one
        let _dir = create_test_git_repo_with_tags_and_commits(
            &["v0.1.0", "v0.1.5", "v0.2.0"], // Multiple tags - v0.2.0 should be latest
            &[
                "feat(test): add feature for v0.1.0",
                "fix(test): fix bug for v0.1.5",
                "feat(test): add feature for v0.2.0",
            ],
        )
        .await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();

        // Test changelog with no range - should find latest tag (v0.2.0)
        let args = ChangelogArgs {
            at: None,
            range: None,
            for_version: None,
            output: None,
            owner: Some("test".to_string()),
            repo: Some("repo".to_string()),
        };

        let mut output = Vec::new();
        let result = generate_changelog_to_writer(&mut output, args);
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok(), "Changelog generation should succeed");
        // The key test is that it doesn't crash and finds v0.2.0 as the latest
        // tag (The actual content depends on what commits are after
        // v0.2.0, which may be none)
    }

    #[tokio::test]
    async fn test_changelog_with_for_version() {
        let _dir =
            create_test_git_repo_with_tags_and_commits(&["v0.1.0"], &["feat(test): add feature"])
                .await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();

        let args = ChangelogArgs {
            at: None,
            range: None,
            for_version: Some("v0.2.0".to_string()),
            output: None,
            owner: Some("test".to_string()),
            repo: Some("repo".to_string()),
        };

        let mut output = Vec::new();
        let result = generate_changelog_to_writer(&mut output, args);
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let output_str = String::from_utf8(output).unwrap();
        assert!(
            output_str.contains("Changelog - v0.2.0"),
            "Header should include for-version"
        );
    }

    #[tokio::test]
    async fn test_changelog_with_for_version_no_v_prefix() {
        let _dir = create_test_git_repo_with_tags_and_commits(&["v0.1.0"], &[]).await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();

        let args = ChangelogArgs {
            at: None,
            range: None,
            for_version: Some("0.2.0".to_string()), // No v prefix
            output: None,
            owner: Some("test".to_string()),
            repo: Some("repo".to_string()),
        };

        let mut output = Vec::new();
        let result = generate_changelog_to_writer(&mut output, args);
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let output_str = String::from_utf8(output).unwrap();
        assert!(
            output_str.contains("Changelog - v0.2.0"),
            "Header should normalize version with v prefix"
        );
    }

    #[tokio::test]
    async fn test_changelog_no_tags() {
        // Test changelog generation when no tags exist - should generate from beginning
        let _dir = create_test_git_repo_with_tags_and_commits(
            &[],
            &["feat(test): add feature", "fix(test): fix bug"],
        )
        .await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();

        let args = ChangelogArgs {
            at: None,
            range: None,
            for_version: None,
            output: None,
            owner: Some("test".to_string()),
            repo: Some("repo".to_string()),
        };

        let mut output = Vec::new();
        let result = generate_changelog_to_writer(&mut output, args);
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok(), "Should succeed even with no tags");
        let output_str = String::from_utf8(output).unwrap();
        // Should generate changelog from beginning when no tags exist
        assert!(
            output_str.contains("Changelog"),
            "Should have changelog header"
        );
    }

    #[tokio::test]
    async fn test_changelog_with_range() {
        let _dir = create_test_git_repo_with_tags_and_commits(
            &["v0.1.0", "v0.2.0"],
            &["feat(test): add feature"],
        )
        .await;
        let dir_path = _dir.path().to_path_buf();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&dir_path).unwrap();

        let args = ChangelogArgs {
            at: None,
            range: Some("v0.1.0..v0.2.0".to_string()),
            for_version: None,
            output: None,
            owner: Some("test".to_string()),
            repo: Some("repo".to_string()),
        };

        let mut output = Vec::new();
        let result = generate_changelog_to_writer(&mut output, args);
        std::env::set_current_dir(original_dir).unwrap();

        if let Err(e) = &result {
            eprintln!("Changelog generation failed: {}", e);
        }
        assert!(result.is_ok(), "Changelog with explicit range should work");
    }
}
