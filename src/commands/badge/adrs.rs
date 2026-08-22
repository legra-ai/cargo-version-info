//! Generate ADRs badge.

use std::io::Write;

use anyhow::Result;
use async_fs_io::try_exists;

/// Show the ADRs badge.
pub async fn badge_adrs(writer: &mut dyn Write, package: &cargo_metadata::Package) -> Result<()> {
    let mut logger = cargo_plugin_utils::logger::Logger::new();
    logger.status("Generating", "ADRs badge");

    let manifest_dir = package
        .manifest_path
        .as_std_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    // Check if docs/adr/ directory exists
    let adr_dir = manifest_dir.join("docs/adr");
    let has_adrs = try_exists(&adr_dir).await?;

    if has_adrs {
        let badge_url = "https://img.shields.io/badge/ADRs-index-informational";
        let badge_markdown = format!("[![ADRs]({})](docs/adr/index.typ)", badge_url);
        writeln!(writer, "{}", badge_markdown)?;
    }

    Ok(())
}
