#![doc = include_str!("../README.md")]

/// Command implementations and argument types.
///
/// # Example: Using in `build.rs` to set `CARGO_PKG_VERSION`
///
/// Add `cargo-version-info` as a build dependency in your `Cargo.toml`:
///
/// ```toml
/// [build-dependencies]
/// cargo-version-info = { version = "0.0.1", default-features = false }
/// ```
///
/// Then in your `build.rs`:
///
/// ```no_run
/// use cargo_version_info::commands::compute_version_string;
///
/// #[tokio::main]
/// async fn main() {
///     if let Ok(version) = compute_version_string(".").await {
///         println!("cargo:rustc-env=CARGO_PKG_VERSION={}", version);
///         println!("cargo:rerun-if-changed=.git/HEAD");
///         println!("cargo:rerun-if-changed=.git/refs");
///     }
/// }
/// ```
///
/// This will override `CARGO_PKG_VERSION` with the computed version based on:
/// 1. `BUILD_VERSION` env var (highest priority, set by CI)
/// 2. GitHub API (in GitHub Actions)
/// 3. Cargo.toml version + git SHA
/// 4. Git SHA fallback (`0.0.0-dev-<sha>`)
pub mod commands;
/// GitHub helpers.
pub mod github;
/// Version helpers.
pub mod version;
