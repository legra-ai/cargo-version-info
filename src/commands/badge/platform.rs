//! Generate platform badge.

use std::io::Write;

use anyhow::Result;
use async_fs_io::{
    read_string_bounded,
    try_exists,
};

/// Show the platform badge.
pub async fn badge_platform(
    writer: &mut dyn Write,
    package: &cargo_metadata::Package,
) -> Result<()> {
    let mut logger = cargo_plugin_utils::logger::Logger::new();
    logger.status("Generating", "platform badge");

    let manifest_dir = package
        .manifest_path
        .as_std_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    // Check for platform indicators
    let has_fly = try_exists(manifest_dir.join("fly.toml")).await?
        || try_exists(manifest_dir.join(".fly")).await?
        || (try_exists(manifest_dir.join("Dockerfile")).await?
            && read_string_bounded(manifest_dir.join("Dockerfile"), 16 * 1024 * 1024)
                .await?
                .contains("fly.io"));

    let has_vercel = try_exists(manifest_dir.join("vercel.json")).await?
        || try_exists(manifest_dir.join(".vercel")).await?;

    if has_fly {
        let badge_url = "https://img.shields.io/badge/platform-Fly.io-8A2BE2";
        let badge_markdown = format!(
            "[![Platform]({})](docs/adr/0002-flyio-oxigraph-provisioning-strategy.typ)",
            badge_url
        );
        writeln!(writer, "{}", badge_markdown)?;
    } else if has_vercel {
        let badge_url = "https://img.shields.io/badge/platform-Vercel-black";
        let badge_markdown = format!("[![Platform]({})](docs/adr/)", badge_url);
        writeln!(writer, "{}", badge_markdown)?;
    }
    // Future: add other platforms (AWS, GCP, Azure, etc.)

    Ok(())
}
