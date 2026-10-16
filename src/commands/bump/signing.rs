//! Validated commit signing, prepared before any version files change.
//!
//! Private-key paths select file signing. Public-key files, `key::` public
//! keys, SHA256 fingerprints and encrypted private keys select SSH-agent
//! signing. A prepared signer never changes backend after an error.

mod agent;
mod payload;
#[cfg(test)]
mod tests;

use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
};
use async_fs_io::read_string_bounded;
use bstr::ByteSlice;
pub use payload::build_commit_payload;
#[cfg(test)]
use payload::write_signature;
use ssh_key::{
    HashAlg,
    PrivateKey,
    PublicKey,
    SshSig,
};

/// The configured Git signing format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SigningFormat {
    /// Git's default OpenPGP format (not supported by this tool).
    #[default]
    Gpg,
    /// SSH signatures.
    Ssh,
}

/// Effective Git commit-signing configuration.
#[derive(Debug, Clone, Default)]
pub struct SigningConfig {
    /// Whether Git commit signing is enabled.
    pub enabled: bool,
    /// Signing format, defaulting to OpenPGP like Git.
    pub format: SigningFormat,
    /// Key path, inline public key, or exact SHA256 fingerprint.
    pub signing_key: Option<String>,
}

/// Read signing configuration without hiding malformed values.
pub fn read_signing_config(repo: &gix::Repository) -> Result<SigningConfig> {
    let config = repo.config_snapshot();
    let enabled = config
        .try_boolean("commit.gpgsign")
        .context("Invalid Git config commit.gpgsign: expected a boolean")?
        .unwrap_or(false);
    if !enabled {
        return Ok(SigningConfig::default());
    }
    let format = match config.string("gpg.format").as_deref().map(|s| s.as_bstr()) {
        None => SigningFormat::Gpg,
        Some(value) if value == "openpgp" => SigningFormat::Gpg,
        Some(value) if value == "ssh" => SigningFormat::Ssh,
        Some(value) => anyhow::bail!("Unsupported Git config gpg.format: {value}"),
    };
    let signing_key = config
        .string("user.signingkey")
        .map(|key| {
            key.to_str()
                .map(str::to_owned)
                .context("user.signingkey must be UTF-8")
        })
        .transpose()?;
    Ok(SigningConfig {
        enabled,
        format,
        signing_key,
    })
}

/// A signer that has successfully signed and verified a preflight challenge.
/// The selected key and backend are retained for the actual commit.
pub struct PreparedSigner {
    backend: Backend,
}

enum Backend {
    Disabled,
    File(Box<PrivateKey>),
    Agent(agent::AgentSigner),
}

impl PreparedSigner {
    /// Resolve effective Git configuration and prove signing works.
    pub async fn for_manifest(manifest: &Path) -> Result<Self> {
        let manifest_dir = manifest
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        // Audited blocking boundary: gix repository discovery/configuration
        // only offers synchronous filesystem I/O. Replace when gix
        // gains async access.
        let (config, root) = tokio::task::spawn_blocking(move || -> Result<_> {
            let repo = gix::discover(manifest_dir)
                .context("Signing preflight: not in a Git repository")?;
            let config = read_signing_config(&repo)?;
            let root = repo
                .workdir()
                .context("Signing requires a working tree")?
                .to_path_buf();
            Ok((config, root))
        })
        .await
        .context("Signing configuration task failed")??;
        let signer = Self::prepare(&config, &root)
            .await
            .context("Signing preflight failed")?;
        signer
            .sign(b"cargo-version-info signing preflight")
            .await
            .context("Signing preflight challenge failed")?;
        Ok(signer)
    }

    async fn prepare(config: &SigningConfig, root: &Path) -> Result<Self> {
        if !config.enabled {
            return Ok(Self {
                backend: Backend::Disabled,
            });
        }
        anyhow::ensure!(
            config.format == SigningFormat::Ssh,
            "OpenPGP signing is not supported; configure gpg.format=ssh and user.signingkey explicitly"
        );
        let key = config
            .signing_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .context("Commit signing is enabled but user.signingkey is missing or empty")?;
        let backend = if let Some(public) = key.strip_prefix("key::") {
            Backend::Agent(
                agent::AgentSigner::for_public(
                    PublicKey::from_openssh(public)
                        .context("Invalid inline SSH public key in user.signingkey")?,
                )
                .await?,
            )
        } else if key.starts_with("SHA256:") {
            Backend::Agent(
                agent::AgentSigner::for_fingerprint(
                    key.parse()
                        .context("Invalid SSH SHA256 fingerprint in user.signingkey")?,
                )
                .await?,
            )
        } else {
            Self::from_path(key, root).await?
        };
        Ok(Self { backend })
    }

    async fn from_path(key: &str, root: &Path) -> Result<Backend> {
        let path = if let Some(relative) = key.strip_prefix("~/") {
            PathBuf::from(
                std::env::var_os("HOME").context("HOME is needed to resolve user.signingkey")?,
            )
            .join(relative)
        } else {
            root.join(key)
        };
        // bounded: OpenSSH key material is capped at 1 MiB.
        let content = read_string_bounded(&path, 1024 * 1024)
            .await
            .with_context(|| {
                format!("Cannot read configured SSH signing key {}", path.display())
            })?;
        if content
            .trim_start()
            .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
        {
            let private = PrivateKey::from_openssh(&content)
                .with_context(|| format!("Invalid SSH private key {}", path.display()))?;
            if private.is_encrypted() {
                return Ok(Backend::Agent(agent::AgentSigner::for_public(private.public_key().clone()).await
                    .with_context(|| format!("Encrypted signing key requires an agent; load it with ssh-add '{}'", path.display()))?));
            }
            return Ok(Backend::File(Box::new(private)));
        }
        let public = PublicKey::from_openssh(&content)
            .with_context(|| format!("Invalid SSH public key {}", path.display()))?;
        Ok(Backend::Agent(
            agent::AgentSigner::for_public(public)
                .await
                .with_context(|| {
                    format!(
                        "Public signing key {} requires an SSH agent",
                        path.display()
                    )
                })?,
        ))
    }

    /// Describe the selected signing backend without exposing key material.
    pub fn description(&self) -> String {
        match &self.backend {
            Backend::Disabled => "disabled by Git configuration".into(),
            Backend::File(key) => format!(
                "SSH key file ({})",
                key.public_key().fingerprint(HashAlg::Sha256)
            ),
            Backend::Agent(agent) => format!(
                "SSH agent ({})",
                agent.public_key().fingerprint(HashAlg::Sha256)
            ),
        }
    }

    /// Sign and verify a payload using the preflight-selected key and backend.
    pub async fn sign(&self, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let (public, signature) = match &self.backend {
            Backend::Disabled => return Ok(None),
            Backend::File(key) => (
                key.public_key(),
                SshSig::sign(key.as_ref(), "git", HashAlg::Sha512, payload)
                    .context("SSH key-file signing failed")?,
            ),
            Backend::Agent(agent) => (agent.public_key(), agent.sign(payload).await?),
        };
        public
            .verify("git", payload, &signature)
            .context("SSH signature verification failed")?;
        Ok(Some(
            signature
                .to_pem(ssh_key::LineEnding::LF)
                .context("Failed to encode SSH signature")?
                .into_bytes(),
        ))
    }
}
