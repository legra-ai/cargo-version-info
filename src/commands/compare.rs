//! Compare two versions command.
//!
//! This command compares two semantic version strings and determines their
//! relationship (greater than, less than, or equal).
//!
//! # Examples
//!
//! ```bash
//! # Compare versions (returns true/false for version1 > version2)
//! cargo version-info compare 0.2.0 0.1.0
//!
//! # Get JSON output
//! cargo version-info compare 0.2.0 0.1.0 --format json
//!
//! # Get human-readable diff
//! cargo version-info compare 0.2.0 0.1.0 --format diff
//! ```

use anyhow::Result;
use clap::Parser;

use crate::version::compare_versions;

/// Arguments for the `compare` command.
#[derive(Parser, Debug)]
pub struct CompareArgs {
    /// First version to compare.
    ///
    /// Can include or omit the 'v' prefix (e.g., "0.1.2" or "v0.1.2").
    version1: String,

    /// Second version to compare.
    ///
    /// Can include or omit the 'v' prefix (e.g., "0.1.2" or "v0.1.2").
    version2: String,

    /// Output format for the comparison result.
    ///
    /// - `bool`: Print "true" if version1 > version2, "false" otherwise
    /// - `json`: Print JSON with result, version1, and version2 fields
    /// - `diff`: Print human-readable comparison (e.g., "0.2.0 > 0.1.0")
    #[arg(long, default_value = "bool")]
    format: String,
}

/// Compare two semantic version strings.
///
/// Determines the relationship between two versions:
/// - Returns `Some(true)` if version1 > version2
/// - Returns `Some(false)` if version1 < version2
/// - Returns `None` if version1 == version2
///
/// # Errors
///
/// Returns an error if either version string cannot be parsed as a valid
/// semantic version (major.minor.patch).
///
/// # Examples
///
/// ```no_run
/// use cargo_version_info::commands::{
///     CompareArgs,
///     compare,
/// };
/// use clap::Parser;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Parse from command line args
/// let args = CompareArgs::parse_from(&["cargo", "version-info", "compare", "0.2.0", "0.1.0"]);
/// compare(args).await?; // Prints "true"
///
/// # Ok(())
/// # }
/// ```
///
/// # Example Output
///
/// With `--format bool` (version1 > version2):
/// ```text
/// true
/// ```
///
/// With `--format bool` (version1 < version2):
/// ```text
/// false
/// ```
///
/// With `--format bool` (version1 == version2):
/// ```text
/// false
/// ```
///
/// With `--format json` (version1 > version2):
/// ```json
/// {"result":"greater","version1":"0.2.0","version2":"0.1.0"}
/// ```
///
/// With `--format json` (version1 < version2):
/// ```json
/// {"result":"less","version1":"0.1.0","version2":"0.2.0"}
/// ```
///
/// With `--format json` (version1 == version2):
/// ```json
/// {"result":"equal","version1":"0.1.0","version2":"0.1.0"}
/// ```
///
/// With `--format diff`:
/// ```text
/// 0.2.0 > 0.1.0
/// ```
///
/// Or:
/// ```text
/// 0.1.0 < 0.2.0
/// ```
///
/// Or:
/// ```text
/// 0.1.0 == 0.1.0
/// ```
pub async fn compare(args: CompareArgs) -> Result<()> {
    let comparison = compare_versions(&args.version1, &args.version2)?;

    match args.format.as_str() {
        "bool" => {
            match comparison {
                Some(true) => println!("true"),   // version1 > version2
                Some(false) => println!("false"), // version1 < version2
                None => println!("false"),        // version1 == version2
            }
        }
        "json" => match comparison {
            Some(true) => println!(
                "{{\"result\":\"greater\",\"version1\":\"{}\",\"version2\":\"{}\"}}",
                args.version1, args.version2
            ),
            Some(false) => println!(
                "{{\"result\":\"less\",\"version1\":\"{}\",\"version2\":\"{}\"}}",
                args.version1, args.version2
            ),
            None => println!(
                "{{\"result\":\"equal\",\"version1\":\"{}\",\"version2\":\"{}\"}}",
                args.version1, args.version2
            ),
        },
        "diff" => match comparison {
            Some(true) => println!("{} > {}", args.version1, args.version2),
            Some(false) => println!("{} < {}", args.version1, args.version2),
            None => println!("{} == {}", args.version1, args.version2),
        },
        _ => anyhow::bail!("Invalid format: {}", args.format),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_compare_version1_greater() {
        let args = CompareArgs {
            version1: "0.2.0".to_string(),
            version2: "0.1.0".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_compare_version1_less() {
        let args = CompareArgs {
            version1: "0.1.0".to_string(),
            version2: "0.2.0".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_compare_versions_equal() {
        let args = CompareArgs {
            version1: "0.1.0".to_string(),
            version2: "0.1.0".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_compare_json_format() {
        let args = CompareArgs {
            version1: "1.0.0".to_string(),
            version2: "0.9.0".to_string(),
            format: "json".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_compare_diff_format() {
        let args = CompareArgs {
            version1: "2.0.0".to_string(),
            version2: "1.0.0".to_string(),
            format: "diff".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_compare_with_v_prefix() {
        let args = CompareArgs {
            version1: "v0.2.0".to_string(),
            version2: "v0.1.0".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_compare_invalid_version1() {
        let args = CompareArgs {
            version1: "invalid".to_string(),
            version2: "0.1.0".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_err());
    }

    #[tokio::test]
    async fn test_compare_invalid_version2() {
        let args = CompareArgs {
            version1: "0.1.0".to_string(),
            version2: "invalid".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_err());
    }

    #[tokio::test]
    async fn test_compare_invalid_format() {
        let args = CompareArgs {
            version1: "0.1.0".to_string(),
            version2: "0.2.0".to_string(),
            format: "invalid".to_string(),
        };
        assert!(compare(args).await.is_err());
    }

    #[tokio::test]
    async fn test_compare_major_difference() {
        let args = CompareArgs {
            version1: "2.0.0".to_string(),
            version2: "1.9.9".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_compare_minor_difference() {
        let args = CompareArgs {
            version1: "1.2.0".to_string(),
            version2: "1.1.9".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }

    #[tokio::test]
    async fn test_compare_patch_difference() {
        let args = CompareArgs {
            version1: "1.1.2".to_string(),
            version2: "1.1.1".to_string(),
            format: "bool".to_string(),
        };
        assert!(compare(args).await.is_ok());
    }
}
