//! Signing must be usable before a bump changes any repository files.

// These fixtures launch the Unix OpenSSH agent with a temporary socket.
#![cfg(unix)]

mod signing_support;

use async_fs_io::{
    read_bounded,
    write_bytes,
};
use signing_support::{
    agent,
    bump,
    git,
    keypair,
    project,
    run_bump,
    verify_commit,
};

#[tokio::test]
async fn signing_preflight_failure_preserves_files_index_head_and_hooks() {
    for (setting, value) in [
        ("user.signingkey", "missing-key"),
        ("gpg.format", "invalid-format"),
        ("commit.gpgsign", "invalid-boolean"),
        ("user.signingkey", ""),
    ] {
        let dir = project().await;
        git(dir.path(), &["config", setting, value]).await;
        let manifest = read_bounded(dir.path().join("Cargo.toml"), 4096)
            .await
            .unwrap();
        let readme = read_bounded(dir.path().join("README.md"), 4096)
            .await
            .unwrap();
        let index = read_bounded(dir.path().join(".git/index"), 65536)
            .await
            .unwrap();
        let head = git(dir.path(), &["rev-parse", "HEAD"]).await;
        let output = bump(dir.path(), false).await;
        assert!(
            !output.status.success(),
            "{setting}={value} unexpectedly succeeded"
        );
        assert_eq!(
            read_bounded(dir.path().join("Cargo.toml"), 4096)
                .await
                .unwrap(),
            manifest,
            "{setting}={value}: manifest changed before signing validation"
        );
        assert_eq!(
            read_bounded(dir.path().join("README.md"), 4096)
                .await
                .unwrap(),
            readme
        );
        assert_eq!(
            read_bounded(dir.path().join(".git/index"), 65536)
                .await
                .unwrap(),
            index
        );
        assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]).await, head);
        assert!(
            !async_fs_io::try_exists(dir.path().join("hook-ran"))
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn signing_preflight_is_skipped_for_no_commit() {
    let dir = project().await;
    git(dir.path(), &["config", "user.signingkey", "missing-key"]).await;
    let output = bump(dir.path(), true).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn private_key_signs_without_agent_and_survives_hook_config_change() {
    let dir = project().await;
    let key = keypair(dir.path(), "signing-key").await;
    git(
        dir.path(),
        &["config", "user.signingkey", key.to_str().unwrap()],
    )
    .await;
    let manifest_path = dir.path().join("Cargo.toml");
    let manifest = async_fs_io::read_string_bounded(&manifest_path, 4096)
        .await
        .unwrap()
        .replace("touch hook-ran", "git config user.signingkey nonexistent");
    write_bytes(&manifest_path, manifest.as_bytes())
        .await
        .unwrap();
    git(dir.path(), &["add", "Cargo.toml"]).await;
    git(
        dir.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "test(fixture): configure hook",
        ],
    )
    .await;
    let output = bump(dir.path(), false).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    verify_commit(dir.path(), &key.with_extension("pub")).await;
}

#[tokio::test]
async fn public_key_requires_agent_even_when_private_key_exists() {
    let dir = project().await;
    let key = keypair(dir.path(), "signing-key").await;
    git(
        dir.path(),
        &[
            "config",
            "user.signingkey",
            key.with_extension("pub").to_str().unwrap(),
        ],
    )
    .await;
    let original = read_bounded(dir.path().join("Cargo.toml"), 4096)
        .await
        .unwrap();
    let output = bump(dir.path(), false).await;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SSH_AUTH_SOCK"));
    assert_eq!(
        read_bounded(dir.path().join("Cargo.toml"), 4096)
            .await
            .unwrap(),
        original
    );
}

#[tokio::test]
async fn agent_signs_public_files_inline_keys_and_exact_fingerprints() {
    for selector in ["file", "inline", "fingerprint"] {
        let dir = project().await;
        let key = keypair(dir.path(), "signing-key").await;
        let public_path = key.with_extension("pub");
        let public = async_fs_io::read_string_bounded(&public_path, 4096)
            .await
            .unwrap();
        let setting = match selector {
            "file" => public_path.to_str().unwrap().to_owned(),
            "inline" => format!("key::{}", public.trim()),
            _ => ssh_key::PublicKey::from_openssh(&public)
                .unwrap()
                .fingerprint(ssh_key::HashAlg::Sha256)
                .to_string(),
        };
        git(dir.path(), &["config", "user.signingkey", &setting]).await;
        let (mut agent, socket) = agent(dir.path(), &key).await;
        let output = run_bump(dir.path(), false, Some(&socket)).await;
        agent.kill().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        verify_commit(dir.path(), &public_path).await;
    }
}

#[tokio::test]
async fn agent_rejects_different_key_with_same_comment() {
    let dir = project().await;
    let expected = keypair(dir.path(), "expected").await;
    let other = keypair(dir.path(), "other").await;
    git(
        dir.path(),
        &[
            "config",
            "user.signingkey",
            expected.with_extension("pub").to_str().unwrap(),
        ],
    )
    .await;
    let (mut agent, socket) = agent(dir.path(), &other).await;
    let original = read_bounded(dir.path().join("Cargo.toml"), 4096)
        .await
        .unwrap();
    let output = run_bump(dir.path(), false, Some(&socket)).await;
    agent.kill().await.unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not loaded in the agent"));
    assert_eq!(
        read_bounded(dir.path().join("Cargo.toml"), 4096)
            .await
            .unwrap(),
        original
    );
}
