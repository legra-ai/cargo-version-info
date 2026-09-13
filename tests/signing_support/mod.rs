//! Isolated repositories and SSH fixtures for signing tests.

use std::path::{
    Path,
    PathBuf,
};

use async_fs_io::{
    TempDir,
    ensure_dir,
    write_bytes,
};
use tokio::process::{
    Child,
    Command,
};

pub async fn git(path: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

pub async fn project() -> TempDir {
    let dir = TempDir::create(std::env::temp_dir()).await.unwrap();
    ensure_dir(dir.path().join("src")).await.unwrap();
    write_bytes(dir.path().join("src/lib.rs"), b"// fixture\n")
        .await
        .unwrap();
    write_bytes(
        dir.path().join("Cargo.toml"),
        br#"[package]
name = "signing-fixture"
version = "0.1.0"
edition = "2024"
[package.metadata.version-info]
pre_bump_hooks = ["touch hook-ran"]
"#,
    )
    .await
    .unwrap();
    write_bytes(
        dir.path().join("README.md"),
        b"signing-fixture = \"0.1.0\"\n",
    )
    .await
    .unwrap();
    git(dir.path(), &["init"]).await;
    git(dir.path(), &["config", "user.name", "Test"]).await;
    git(dir.path(), &["config", "user.email", "test@example.com"]).await;
    git(dir.path(), &["config", "commit.gpgsign", "false"]).await;
    git(dir.path(), &["add", "."]).await;
    git(dir.path(), &["commit", "-m", "test(fixture): initial"]).await;
    git(dir.path(), &["config", "commit.gpgsign", "true"]).await;
    git(dir.path(), &["config", "gpg.format", "ssh"]).await;
    dir
}

pub async fn bump(path: &Path, no_commit: bool) -> std::process::Output {
    run_bump(path, no_commit, None).await
}

pub async fn run_bump(path: &Path, no_commit: bool, socket: Option<&Path>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-version-info"));
    command
        .args(["version-info", "bump", "--patch", "--no-lock"])
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("SSH_AUTH_SOCK");
    if let Some(socket) = socket {
        command.env("SSH_AUTH_SOCK", socket);
    }
    if no_commit {
        command.arg("--no-commit");
    }
    command.output().await.unwrap()
}

pub async fn keypair(path: &Path, name: &str) -> PathBuf {
    let key = path.join(name);
    let output = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "same-comment", "-f"])
        .arg(&key)
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    key
}

pub async fn agent(path: &Path, key: &Path) -> (Child, PathBuf) {
    let socket = path.join("agent.sock");
    let mut agent = Command::new("ssh-agent")
        .args(["-D", "-a"])
        .arg(&socket)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if async_fs_io::try_exists(&socket).await.unwrap() {
            break;
        }
        assert!(
            agent.try_wait().unwrap().is_none(),
            "ssh-agent exited before ready"
        );
        // Yield while the child creates its socket; no synchronous filesystem I/O.
        tokio::task::yield_now().await;
        let _ = Command::new("sleep").arg("0.01").status().await.unwrap();
    }
    let output = Command::new("ssh-add")
        .arg(key)
        .env("SSH_AUTH_SOCK", &socket)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (agent, socket)
}

pub async fn verify_commit(path: &Path, public_path: &Path) {
    let public = async_fs_io::read_string_bounded(public_path, 4096)
        .await
        .unwrap();
    let allowed = path.join("allowed-signers");
    write_bytes(&allowed, format!("test@example.com {public}").as_bytes())
        .await
        .unwrap();
    git(
        path,
        &[
            "config",
            "gpg.ssh.allowedSignersFile",
            allowed.to_str().unwrap(),
        ],
    )
    .await;
    git(path, &["verify-commit", "HEAD"]).await;
}
