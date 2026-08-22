//! Get Dioxus version from Cargo.toml command.
//!
//! This command extracts the Dioxus framework version from a Cargo.toml
//! manifest file, handling both direct version specifications and workspace
//! inheritance.
//!
//! # Examples
//!
//! ```bash
//! # Get Dioxus version (e.g., "0.7.0")
//! cargo version-info dioxus
//!
//! # Get JSON output
//! cargo version-info dioxus --format json
//!
//! # Use a different Cargo.toml
//! cargo version-info dioxus --manifest ./path/to/Cargo.toml
//! ```

use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use async_fs_io::read_string_bounded;
use clap::Parser;

/// Arguments for the `dioxus` command.
#[derive(Parser, Debug)]
pub struct DioxusArgs {
    /// Path to the Cargo.toml manifest file.
    ///
    /// Defaults to `./Cargo.toml` in the current directory.
    #[arg(long, default_value = "./Cargo.toml")]
    manifest: PathBuf,

    /// Output format for the Dioxus version.
    ///
    /// - `version`: Print just the version number (e.g., "0.7.0")
    /// - `json`: Print JSON with version field
    #[arg(long, default_value = "version")]
    format: String,
}

/// Get the Dioxus framework version from a Cargo.toml manifest file.
///
/// Extracts the version from the `dioxus` dependency in `[dependencies]`,
/// handling both direct version specifications (`dioxus = { version = "..." }`)
/// and workspace inheritance (`dioxus = { workspace = true }`). For workspace
/// dependencies, falls back to `workspace.package.version`.
///
/// # Errors
///
/// Returns an error if:
/// - The manifest file cannot be read or parsed
/// - The `dioxus` dependency is not found
/// - The dependency uses workspace inheritance but no workspace version is
///   found
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     DioxusArgs,
///     dioxus,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = DioxusArgs::parse_from(&["cargo", "version-info", "dioxus"]);
/// dioxus(args).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Example Output
///
/// With `--format version`:
/// ```text
/// 0.7.0
/// ```
///
/// With `--format json`:
/// ```json
/// {"version":"0.7.0"}
/// ```
pub async fn dioxus(args: DioxusArgs) -> Result<()> {
    let content = read_string_bounded(&args.manifest, 16 * 1024 * 1024)
        .await
        .with_context(|| format!("Failed to read {}", args.manifest.display()))?;

    // Parse dioxus = { version = "..." } or dioxus = { workspace = true }
    // Use toml crate for proper parsing
    let parsed: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", args.manifest.display()))?;

    let version = parsed
        .get("dependencies")
        .and_then(|deps| deps.get("dioxus"))
        .and_then(|dioxus_val| {
            if let Some(version_str) = dioxus_val.get("version").and_then(|v| v.as_str()) {
                Some(version_str.to_string())
            } else if dioxus_val.get("workspace").is_some() {
                // Check workspace.package.version
                parsed
                    .get("workspace")
                    .and_then(|ws| ws.get("package"))
                    .and_then(|pkg| pkg.get("version"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        })
        .with_context(|| format!("No dioxus version found in {}", args.manifest.display()))?;

    match args.format.as_str() {
        "version" => println!("{}", version),
        "json" => println!("{{\"version\":\"{}\"}}", version),
        _ => anyhow::bail!("Invalid format: {}", args.format),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_temp_manifest(content: &str) -> async_fs_io::TempFile {
        let file = async_fs_io::TempFile::create(std::env::temp_dir())
            .await
            .unwrap();
        async_fs_io::write_bytes(file.path(), content.as_bytes())
            .await
            .unwrap();
        file
    }

    #[tokio::test]
    async fn test_dioxus_direct_version() {
        let manifest = create_temp_manifest(
            r#"
[dependencies]
dioxus = { version = "0.7.0" }
"#,
        )
        .await;
        let args = DioxusArgs {
            manifest: manifest.path().to_path_buf(),
            format: "version".to_string(),
        };
        assert!(dioxus(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_dioxus_workspace_inheritance() {
        let manifest = create_temp_manifest(
            r#"
[workspace.package]
version = "0.8.0"

[dependencies]
dioxus = { workspace = true }
"#,
        )
        .await;
        let args = DioxusArgs {
            manifest: manifest.path().to_path_buf(),
            format: "version".to_string(),
        };
        assert!(dioxus(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_dioxus_json_format() {
        let manifest = create_temp_manifest(
            r#"
[dependencies]
dioxus = { version = "0.9.0" }
"#,
        )
        .await;
        let args = DioxusArgs {
            manifest: manifest.path().to_path_buf(),
            format: "json".to_string(),
        };
        assert!(dioxus(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_dioxus_not_found() {
        let manifest = create_temp_manifest(
            r#"
[dependencies]
serde = { version = "1.0" }
"#,
        )
        .await;
        let args = DioxusArgs {
            manifest: manifest.path().to_path_buf(),
            format: "version".to_string(),
        };
        assert!(dioxus(args).await.is_err());
    }

    #[tokio::test]
    async fn test_dioxus_file_not_found() {
        let args = DioxusArgs {
            manifest: "/nonexistent/Cargo.toml".into(),
            format: "version".to_string(),
        };
        assert!(dioxus(args).await.is_err());
    }

    #[tokio::test]
    async fn test_dioxus_invalid_format() {
        let manifest = create_temp_manifest(
            r#"
[dependencies]
dioxus = { version = "0.7.0" }
"#,
        )
        .await;
        let args = DioxusArgs {
            manifest: manifest.path().to_path_buf(),
            format: "invalid".to_string(),
        };
        assert!(dioxus(args).await.is_err());
    }

    #[tokio::test]
    async fn test_dioxus_invalid_toml() {
        let manifest = create_temp_manifest(
            r#"
[invalid toml content
"#,
        )
        .await;
        let args = DioxusArgs {
            manifest: manifest.path().to_path_buf(),
            format: "version".to_string(),
        };
        assert!(dioxus(args).await.is_err());
    }
}
