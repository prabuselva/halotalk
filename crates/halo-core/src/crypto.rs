//! At-rest encryption for provider API keys (ChaCha20-Poly1305, `xc1:` prefix).
//! Key source: `HALO_SECRET` env var (sha256-derived) or a generated
//! `<data_dir>/secret.key` (mode 0600, base64url 32 bytes). Values without a
//! known prefix are treated as legacy plaintext and passed through untouched;
//! legacy fernet `fer1:` values (pre pure-Rust crypto) decrypt to empty, the
//! same key-rotation behavior that makes the user re-enter the key.
// aead 0.5 re-exports generic-array 0.14, which is deprecated upstream; drop
// this allow when chacha20poly1305 moves to generic-array 1.x.
#![allow(deprecated)]

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, Payload, generic_array::GenericArray},
};
use sha2::{Digest as _, Sha256};

pub const CIPHER_PREFIX: &str = "xc1:";
const LEGACY_PREFIX: &str = "fer1:";
const NONCE_LEN: usize = 12;

pub fn is_cipher(stored: &str) -> bool {
    stored.starts_with(CIPHER_PREFIX)
}

fn random<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::getrandom(&mut buf).expect("system RNG unavailable");
    buf
}

#[derive(Clone)]
pub struct Cipher(ChaCha20Poly1305);

impl Cipher {
    fn from_key_bytes(key: [u8; 32]) -> Self {
        Self(ChaCha20Poly1305::new(&key.into()))
    }

    pub fn from_secret(secret: &str) -> Self {
        Self::from_key_bytes(Sha256::digest(secret.as_bytes()).into())
    }

    /// Load the app key: `HALO_SECRET` wins; else read or create `secret.key`.
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        if let Some(secret) = std::env::var("HALO_SECRET").ok().filter(|s| !s.is_empty()) {
            return Ok(Self::from_secret(&secret));
        }
        let path = data_dir.join("secret.key");
        if let Ok(existing) = fs::read_to_string(&path) {
            let existing = existing.trim();
            if !existing.is_empty() {
                let raw = URL_SAFE
                    .decode(existing)
                    .with_context(|| format!("invalid key in {}", path.display()))?;
                let key: [u8; 32] = raw
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("key in {} must be 32 bytes", path.display()))?;
                return Ok(Self::from_key_bytes(key));
            }
        }
        let key = random::<32>();
        fs::create_dir_all(data_dir)
            .with_context(|| format!("cannot create {}", data_dir.display()))?;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path);
        let mut file = match file {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!(
                    "cannot initialize {}; another instance is writing it",
                    path.display()
                );
            }
            Err(e) => return Err(e).with_context(|| format!("cannot write {}", path.display())),
        };
        file.write_all(URL_SAFE.encode(key).as_bytes())?;
        drop(file);
        Ok(Self::from_key_bytes(key))
    }

    pub fn encrypt(&self, plain: &str) -> String {
        let nonce = random::<NONCE_LEN>();
        let ct = self
            .0
            .encrypt(GenericArray::from_slice(&nonce), plain.as_bytes())
            .expect("encrypt with a valid key cannot fail");
        format!(
            "{CIPHER_PREFIX}{}",
            URL_SAFE.encode([nonce.as_slice(), ct.as_slice()].concat())
        )
    }

    /// Undecryptable (e.g. key-rotated or legacy fernet) values decrypt to
    /// empty, forcing the user to re-enter the key rather than leaking the
    /// ciphertext downstream.
    pub fn decrypt(&self, stored: &str) -> String {
        if stored.starts_with(LEGACY_PREFIX) {
            return String::new();
        }
        let Some(token) = stored.strip_prefix(CIPHER_PREFIX) else {
            return stored.to_string();
        };
        let Ok(raw) = URL_SAFE.decode(token) else {
            return String::new();
        };
        let Some((nonce, ct)) = raw.split_at_checked(NONCE_LEN) else {
            return String::new();
        };
        match self.0.decrypt(
            GenericArray::from_slice(nonce),
            Payload { msg: ct, aad: &[] },
        ) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_from_secret() {
        let a = Cipher::from_secret("s3cret");
        let b = Cipher::from_secret("s3cret");
        let ct = a.encrypt("sk-abc");
        assert!(is_cipher(&ct));
        assert_eq!(b.decrypt(&ct), "sk-abc");
    }

    #[test]
    fn wrong_secret_yields_empty_and_plaintext_passthrough() {
        let a = Cipher::from_secret("a");
        let b = Cipher::from_secret("b");
        let ct = a.encrypt("sk-abc");
        assert_eq!(b.decrypt(&ct), "");
        assert_eq!(a.decrypt("plain-key"), "plain-key");
        assert!(!is_cipher("plain-key"));
    }

    #[test]
    fn legacy_fernet_and_tampered_tokens_decrypt_empty() {
        let a = Cipher::from_secret("a");
        assert_eq!(a.decrypt("fer1:someoldtoken"), "");
        assert!(!is_cipher("fer1:someoldtoken"));
        let mut ct = a.encrypt("sk-abc");
        ct.push('x');
        assert_eq!(a.decrypt(&ct), "");
    }
}
