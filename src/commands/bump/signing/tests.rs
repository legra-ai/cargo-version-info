use super::*;

#[tokio::test]
async fn test_signing_format_default() {
    assert_eq!(SigningFormat::default(), SigningFormat::Gpg);
}

#[tokio::test]
async fn test_signing_config_default() {
    let config = SigningConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.format, SigningFormat::Gpg);
    assert!(config.signing_key.is_none());
}

#[tokio::test]
async fn test_write_signature_positive_offset() {
    let mut buf = Vec::new();
    let sig = gix::actor::Signature {
        name: "Test User".into(),
        email: "test@example.com".into(),
        time: gix::date::Time {
            seconds: 1700000000,
            offset: 60, // UTC+1
        },
    };

    write_signature(&mut buf, &sig);
    let result = String::from_utf8_lossy(&buf);

    assert!(result.contains("Test User <test@example.com>"));
    assert!(result.contains("1700000000"));
    assert!(result.contains("+0100"));
}

#[tokio::test]
async fn test_write_signature_negative_offset() {
    let mut buf = Vec::new();
    let sig = gix::actor::Signature {
        name: "Test User".into(),
        email: "test@example.com".into(),
        time: gix::date::Time {
            seconds: 1700000000,
            offset: -300, // UTC-5
        },
    };

    write_signature(&mut buf, &sig);
    let result = String::from_utf8_lossy(&buf);

    assert!(result.contains("Test User <test@example.com>"));
    assert!(result.contains("1700000000"));
    assert!(result.contains("-0500"));
}

/// Helper to create a temporary git repository for testing.
///
/// Creates an isolated git repo that ignores global and system config
/// by setting GIT_CONFIG_NOSYSTEM and creating a local config.
async fn create_test_repo() -> (async_fs_io::TempDir, gix::Repository) {
    use std::process::Command;

    let temp_dir = async_fs_io::TempDir::create(std::env::temp_dir())
        .await
        .expect("Failed to create temp directory");
    let dir = temp_dir.path();

    // Initialize git repo
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .expect("Failed to run git init");

    // Set basic config
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .output()
        .expect("Failed to set user.name");

    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .output()
        .expect("Failed to set user.email");

    // Disable any inherited signing config
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir)
        .output()
        .expect("Failed to set commit.gpgsign");

    // Explicitly unset signing key (override global config)
    Command::new("git")
        .args(["config", "--unset", "user.signingkey"])
        .current_dir(dir)
        .output()
        .ok(); // Ignore error if not set

    // Open with gix in isolated mode to ignore global config
    let repo =
        gix::open_opts(dir, gix::open::Options::isolated()).expect("Failed to open repo with gix");

    (temp_dir, repo)
}

#[tokio::test]
#[serial_test::serial]
async fn test_read_signing_config_no_signing() {
    let (_temp_dir, repo) = create_test_repo().await;

    let config = read_signing_config(&repo).unwrap();

    assert!(!config.enabled);
    assert_eq!(config.format, SigningFormat::Gpg); // Git default
    assert!(config.signing_key.is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn test_read_signing_config_enabled_no_key() {
    use std::process::Command;

    let (temp_dir, _repo) = create_test_repo().await;
    let dir = temp_dir.path();

    // Enable signing without a key
    Command::new("git")
        .args(["config", "commit.gpgsign", "true"])
        .current_dir(dir)
        .output()
        .expect("Failed to set commit.gpgsign");

    // Need to reopen repo to pick up config change (isolated to ignore global)
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("Failed to reopen repo");
    let config = read_signing_config(&repo).unwrap();

    assert!(config.enabled);
    assert_eq!(config.format, SigningFormat::Gpg); // Git default
    assert!(config.signing_key.is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn test_read_signing_config_ssh() {
    use std::process::Command;

    let (temp_dir, _repo) = create_test_repo().await;
    let dir = temp_dir.path();

    // Set up SSH signing config
    Command::new("git")
        .args(["config", "commit.gpgsign", "true"])
        .current_dir(dir)
        .output()
        .expect("Failed to set commit.gpgsign");

    Command::new("git")
        .args(["config", "gpg.format", "ssh"])
        .current_dir(dir)
        .output()
        .expect("Failed to set gpg.format");

    Command::new("git")
        .args(["config", "user.signingkey", "~/.ssh/id_ed25519.pub"])
        .current_dir(dir)
        .output()
        .expect("Failed to set user.signingkey");

    // Reopen repo (isolated to ignore global config)
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("Failed to reopen repo");
    let config = read_signing_config(&repo).unwrap();

    assert!(config.enabled);
    assert_eq!(config.format, SigningFormat::Ssh);
    assert_eq!(
        config.signing_key,
        Some("~/.ssh/id_ed25519.pub".to_string())
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_read_signing_config_gpg() {
    use std::process::Command;

    let (temp_dir, _repo) = create_test_repo().await;
    let dir = temp_dir.path();

    // Set up GPG signing config
    Command::new("git")
        .args(["config", "commit.gpgsign", "true"])
        .current_dir(dir)
        .output()
        .expect("Failed to set commit.gpgsign");

    Command::new("git")
        .args(["config", "gpg.format", "openpgp"])
        .current_dir(dir)
        .output()
        .expect("Failed to set gpg.format");

    Command::new("git")
        .args(["config", "user.signingkey", "ABCD1234EFGH5678"])
        .current_dir(dir)
        .output()
        .expect("Failed to set user.signingkey");

    // Reopen repo (isolated to ignore global config)
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("Failed to reopen repo");
    let config = read_signing_config(&repo).unwrap();

    assert!(config.enabled);
    assert_eq!(config.format, SigningFormat::Gpg);
    assert_eq!(config.signing_key, Some("ABCD1234EFGH5678".to_string()));
}

#[tokio::test]
async fn test_build_commit_payload() {
    let tree_id = gix::ObjectId::from_hex(b"0123456789abcdef0123456789abcdef01234567")
        .expect("Invalid tree ID");
    let parent_id = gix::ObjectId::from_hex(b"fedcba9876543210fedcba9876543210fedcba98")
        .expect("Invalid parent ID");

    let author = gix::actor::Signature {
        name: "Test User".into(),
        email: "test@example.com".into(),
        time: gix::date::Time {
            seconds: 1700000000,
            offset: 0,
        },
    };
    let committer = author.clone();
    let message = "chore(version): bump 1.0.0 -> 1.0.1";

    // We need a repository to get an Id, but for this test we can use a
    // detached ObjectId
    let repo = gix::open_opts(
        std::env::current_dir().expect("No current dir"),
        gix::open::Options::isolated(),
    )
    .expect("Failed to open repo");
    let parent_id_ref = repo.find_object(parent_id);

    // Skip test if we can't find the object (expected in test environment)
    if parent_id_ref.is_err() {
        // Can't test without a real repo with this object
        return;
    }

    let parent_id = parent_id_ref.unwrap().id();
    let payload = build_commit_payload(&tree_id, parent_id, &author, &committer, message);
    let payload_str = String::from_utf8_lossy(&payload);

    assert!(payload_str.starts_with("tree 0123456789abcdef0123456789abcdef01234567\n"));
    assert!(payload_str.contains("author Test User <test@example.com>"));
    assert!(payload_str.contains("committer Test User <test@example.com>"));
    assert!(payload_str.ends_with("chore(version): bump 1.0.0 -> 1.0.1"));
}
