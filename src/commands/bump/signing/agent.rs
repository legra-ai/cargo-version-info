//! Exact SSH-agent key selection and signatures.

use std::path::{
    Path,
    PathBuf,
};
#[cfg(unix)]
use std::time::Duration;

use anyhow::{
    Context,
    Result,
};
use ssh_agent_client_rs::{
    Client,
    Identity,
};
use ssh_key::sha2::{
    Digest,
    Sha512,
};
use ssh_key::{
    Fingerprint,
    HashAlg,
    PublicKey,
    SshSig,
};

pub(super) struct AgentSigner {
    socket: PathBuf,
    public: PublicKey,
    identity: Identity<'static>,
}

impl AgentSigner {
    pub(super) async fn for_public(public: PublicKey) -> Result<Self> {
        Self::for_fingerprint(public.fingerprint(HashAlg::Sha256)).await
    }

    pub(super) async fn for_fingerprint(fingerprint: Fingerprint) -> Result<Self> {
        let socket = PathBuf::from(std::env::var_os("SSH_AUTH_SOCK")
            .context("SSH_AUTH_SOCK is not set; start an SSH agent and load the configured signing key with ssh-add")?);
        let connection_path = socket.clone();
        // Audited blocking boundary: ssh-agent-client-rs exposes only synchronous
        // socket I/O. Replace this adapter when it gains an asynchronous transport.
        let (identity, public) = tokio::task::spawn_blocking(move || {
            let mut client = Self::connect(&connection_path)?;
            // bounded: identities belong to the local SSH agent, not repository data.
            let identities = client
                .list_all_identities()
                .context("Cannot list SSH-agent identities")?;
            Self::select_identity(identities, fingerprint)
        })
        .await
        .context("SSH-agent identity task failed")??;
        Ok(Self {
            socket,
            public,
            identity,
        })
    }

    #[cfg(unix)]
    fn connect(path: &Path) -> Result<Client> {
        let socket = std::os::unix::net::UnixStream::connect(path)
            .with_context(|| format!("Cannot connect to SSH agent at {}", path.display()))?;
        socket.set_read_timeout(Some(Duration::from_secs(15)))?;
        socket.set_write_timeout(Some(Duration::from_secs(15)))?;
        Ok(Client::with_read_write(Box::new(socket)))
    }

    #[cfg(windows)]
    fn connect(path: &Path) -> Result<Client> {
        Client::connect(path)
            .with_context(|| format!("Cannot connect to SSH agent at {}", path.display()))
    }

    fn select_identity(
        identities: Vec<Identity<'static>>,
        fingerprint: Fingerprint,
    ) -> Result<(Identity<'static>, PublicKey)> {
        // bounded: diagnostics cover only identities loaded into the local agent.
        let mut available = String::new();
        for identity in identities {
            let public = match &identity {
                Identity::PublicKey(key) => key.as_ref().as_ref().clone(),
                Identity::Certificate(cert) => PublicKey::new(cert.public_key().clone(), ""),
            };
            let candidate = public.fingerprint(HashAlg::Sha256);
            if candidate == fingerprint {
                return Ok((identity, public));
            }
            available.push_str(&format!("\n  {candidate} {}", public.comment()));
        }
        anyhow::bail!(
            "Configured SSH signing key {fingerprint} is not loaded in the agent. Load that key with ssh-add. Available fingerprints:{available}"
        )
    }

    pub(super) fn public_key(&self) -> &PublicKey {
        &self.public
    }

    pub(super) async fn sign(&self, payload: &[u8]) -> Result<SshSig> {
        let socket = self.socket.clone();
        let identity = self.identity.clone();
        let public = self.public.clone();
        // bounded: SSHSIG signs a fixed-size SHA512 digest plus protocol fields.
        let mut blob = Vec::with_capacity(128);
        blob.extend_from_slice(b"SSHSIG");
        for field in [b"git".as_slice(), b"", b"sha512", &Sha512::digest(payload)] {
            blob.extend_from_slice(&(field.len() as u32).to_be_bytes());
            blob.extend_from_slice(field);
        }
        // Audited blocking boundary: same synchronous agent transport as discovery.
        tokio::task::spawn_blocking(move || {
            let mut client = Self::connect(&socket)?;
            let signature = client
                .sign(identity, &blob)
                .context("Selected SSH agent failed to sign; no backend switch attempted")?;
            SshSig::new(public.key_data().clone(), "git", HashAlg::Sha512, signature)
                .context("Invalid SSH-agent signature")
        })
        .await
        .context("SSH-agent signing task failed")?
    }
}
