//! Generate tag name from version command.
//!
//! This command converts a semantic version string into a git tag name
//! by adding the 'v' prefix.
//!
//! # Examples
//!
//! ```bash
//! # Generate tag (e.g., "v0.1.2")
//! cargo version-info tag 0.1.2
//!
//! # Get JSON output
//! cargo version-info tag 0.1.2 --format json
//!
//! # Works with 'v' prefix already present
//! cargo version-info tag v0.1.2
//! ```

use anyhow::Result;
use clap::Parser;

use crate::version::{
    format_tag,
    parse_version,
};

/// Arguments for the `tag` command.
#[derive(Parser, Debug)]
pub struct TagArgs {
    /// Semantic version string to convert to a tag.
    ///
    /// Can include or omit the 'v' prefix (e.g., "0.1.2" or "v0.1.2").
    /// The output will always include the 'v' prefix.
    version: String,

    /// Output format for the tag.
    ///
    /// - `tag`: Print just the tag (e.g., "v0.1.2")
    /// - `json`: Print JSON with tag and version fields
    #[arg(long, default_value = "tag")]
    format: String,
}

/// Generate a git tag name from a semantic version string.
///
/// Parses the version string and formats it as a git tag with the 'v' prefix.
/// The input version can optionally include the 'v' prefix; it will be stripped
/// and re-added to ensure consistent formatting.
///
/// # Errors
///
/// Returns an error if the version string cannot be parsed as a valid
/// semantic version (major.minor.patch).
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     TagArgs,
///     tag,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = TagArgs::parse_from(&["cargo", "version-info", "tag", "0.1.2"]);
/// tag(args).await?; // Prints "v0.1.2"
///
/// # Ok(())
/// # }
/// ```
///
/// # Example Output
///
/// With `--format tag`:
/// ```text
/// v0.1.2
/// ```
///
/// With `--format json`:
/// ```json
/// {"tag":"v0.1.2","version":"0.1.2"}
/// ```
pub async fn tag(args: TagArgs) -> Result<()> {
    let (major, minor, patch) = parse_version(&args.version)?;
    let tag = format_tag(major, minor, patch);

    match args.format.as_str() {
        "tag" => println!("{}", tag),
        "json" => println!("{{\"tag\":\"{}\",\"version\":\"{}\"}}", tag, args.version),
        _ => anyhow::bail!("Invalid format: {}", args.format),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tag_version_format() {
        let args = TagArgs {
            version: "0.1.2".to_string(),
            format: "tag".to_string(),
        };
        assert!(tag(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_tag_with_v_prefix() {
        let args = TagArgs {
            version: "v0.1.2".to_string(),
            format: "tag".to_string(),
        };
        assert!(tag(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_tag_json_format() {
        let args = TagArgs {
            version: "1.2.3".to_string(),
            format: "json".to_string(),
        };
        assert!(tag(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_tag_invalid_version() {
        let args = TagArgs {
            version: "invalid".to_string(),
            format: "tag".to_string(),
        };
        assert!(tag(args).await.is_err());
    }

    #[tokio::test]
    async fn test_tag_invalid_format() {
        let args = TagArgs {
            version: "0.1.2".to_string(),
            format: "invalid".to_string(),
        };
        assert!(tag(args).await.is_err());
    }

    #[tokio::test]
    async fn test_tag_major_version() {
        let args = TagArgs {
            version: "10.20.30".to_string(),
            format: "tag".to_string(),
        };
        assert!(tag(args).await.is_ok());
    }
}
