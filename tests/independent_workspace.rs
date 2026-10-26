//! A root package release must not change independently versioned members.

use std::env;

use async_fs_io::{
    TempDir,
    ensure_dir,
    read_string_bounded,
    try_exists,
    write_bytes,
};
use tokio::process::Command;

#[tokio::test]
async fn root_bump_preserves_independent_workspace_and_lock_versions() {
    let directory = TempDir::create(env::temp_dir()).await.unwrap();
    let root = directory.path();
    ensure_dir(root.join("src")).await.unwrap();
    ensure_dir(root.join("child/src")).await.unwrap();
    let child = b"[package]\nname='component'\nversion.workspace=true\nedition='2024'\n";
    write_bytes(root.join("child/Cargo.toml"), child)
        .await
        .unwrap();
    write_bytes(root.join("src/lib.rs"), b"//! Root fixture.\n")
        .await
        .unwrap();
    write_bytes(root.join("child/src/lib.rs"), b"//! Child fixture.\n")
        .await
        .unwrap();
    write_bytes(
        root.join("Cargo.toml"),
        b"[package]\nname='fixture'\nversion='0.2.0-alpha.19'\nedition='2024'\n\
          [workspace]\nmembers=['child']\n\
          [workspace.package]\nversion='0.7.0'\n\
          [dependencies]\ncomponent={path='child'}\n",
    )
    .await
    .unwrap();
    let initial = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .output()
        .await
        .unwrap();
    assert!(
        initial.status.success(),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let result = Command::new(env!("CARGO_BIN_EXE_cargo-version-info"))
        .args([
            "version-info",
            "bump",
            "--version",
            "0.2.0-alpha.20",
            "--no-commit",
            "--no-readme",
        ])
        .env("CARGO_NET_OFFLINE", "true")
        .current_dir(root)
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let manifest = read_string_bounded(root.join("Cargo.toml"), 64 * 1024)
        .await
        .unwrap();
    let manifest: toml::Table = toml::from_str(&manifest).unwrap();
    assert_eq!(
        manifest["package"]["version"].as_str(),
        Some("0.2.0-alpha.20")
    );
    assert_eq!(
        manifest["workspace"]["package"]["version"].as_str(),
        Some("0.7.0")
    );
    assert_eq!(
        read_string_bounded(root.join("child/Cargo.toml"), 64 * 1024)
            .await
            .unwrap()
            .as_bytes(),
        child
    );
    let lock = read_string_bounded(root.join("Cargo.lock"), 64 * 1024)
        .await
        .unwrap();
    let lock: toml::Table = toml::from_str(&lock).unwrap();
    // bounded: this fixture has exactly two local packages and no registry
    // dependencies.
    let packages = lock["package"].as_array().unwrap();
    assert_eq!(packages.len(), 2);
    for (name, version) in [("fixture", "0.2.0-alpha.20"), ("component", "0.7.0")] {
        let package = packages
            .iter()
            .find(|package| package["name"].as_str() == Some(name))
            .unwrap();
        assert_eq!(package["version"].as_str(), Some(version));
    }
    assert!(!try_exists(root.join(".git")).await.unwrap());
}
