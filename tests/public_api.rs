//! Public installation checks shipped inside the crate archive.

use std::env;

use async_fs_io::{
    TempDir,
    ensure_dir,
    write_bytes,
};
use tokio::process::Command;

#[tokio::test]
async fn tool_version_is_embedded_and_independent_of_the_target_repository() {
    let directory = TempDir::create(env::temp_dir())
        .await
        .expect("temporary target repository");
    let result = Command::new(env!("CARGO_BIN_EXE_cargo-version-info"))
        .args(["version-info", "--tool-version"])
        .env("BUILD_VERSION", "99.88.77")
        .env("CARGO_PKG_VERSION_OVERRIDE", "88.77.66")
        .current_dir(directory.path())
        .output()
        .await
        .expect("run installed tool");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let expected = format!(
        "cargo-version-info {}.{}.{}\n",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    assert_eq!(
        String::from_utf8(result.stdout).expect("version text"),
        expected
    );
}

#[tokio::test]
async fn current_reads_an_explicit_package_manifest_outside_git() {
    let directory = TempDir::create(env::temp_dir())
        .await
        .expect("temporary package");
    ensure_dir(directory.path().join("src"))
        .await
        .expect("source directory");
    write_bytes(
        directory.path().join("Cargo.toml"),
        b"[package]\nname = 'version-fixture'\nversion = '7.8.9'\nedition = '2024'\n",
    )
    .await
    .expect("manifest");
    write_bytes(
        directory.path().join("src/lib.rs"),
        b"//! Fixture package.\n",
    )
    .await
    .expect("library");
    let result = Command::new(env!("CARGO_BIN_EXE_cargo-version-info"))
        .args(["version-info", "current", "--manifest-path"])
        .arg(directory.path().join("Cargo.toml"))
        .current_dir(directory.path())
        .output()
        .await
        .expect("read version");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8(result.stdout)
            .expect("version text")
            .trim(),
        "7.8.9"
    );
}
