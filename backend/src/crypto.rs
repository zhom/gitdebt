//! AES-256-GCM authenticated encryption for OAuth tokens at rest.
//!
//! Threat model: a Postgres dump leaks `app_users.access_token`. Without
//! encryption, every leaked token is immediately usable to act as that
//! user on GitHub. With this module, the dump is useless without
//! `TOKEN_ENCRYPTION_KEY` (which lives in the runtime env / secret
//! store, not in the database).
//!
//! On-disk format per ciphertext: `b64(version || nonce || ciphertext)`.
//! The 1-byte version prefix lets us rotate algorithms in the future
//! (right now `0x01` is "AES-256-GCM, 12-byte nonce, BEYOND-the-tag").
//!
//! Key handling:
//!   - Required env: `TOKEN_ENCRYPTION_KEY` = 32 raw bytes, base64-encoded.
//!     Generate once with `openssl rand -base64 32`.
//!   - Loading is done at startup; if the OAuth feature is configured
//!     but the key is missing, `main.rs` refuses to start (better than
//!     silently writing plaintext).
//!   - Rotation isn't built-in. When you rotate, decrypt-with-old then
//!     re-encrypt-with-new, then drop the old key. Don't try to support
//!     two keys at once until the need arises.

use std::sync::Arc;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use rand::Rng;

const VERSION_AES_GCM_V1: u8 = 0x01;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

#[derive(Clone)]
pub struct Crypto {
    cipher: Arc<Aes256Gcm>,
}

impl Crypto {
    /// Load from env. Returns `Ok(None)` if `TOKEN_ENCRYPTION_KEY` is
    /// unset (caller decides whether that's fatal); returns `Err` if it
    /// IS set but malformed.
    ///
    /// Accepts the key in either format:
    ///   - base64 (44 chars from `openssl rand -base64 32`)
    ///   - hex    (64 chars from `openssl rand -hex 32`)
    ///
    /// Both decode to the same 32 raw bytes; the env file uses whichever
    /// the user prefers.
    pub fn from_env() -> Result<Option<Self>> {
        let Some(raw) = std::env::var("TOKEN_ENCRYPTION_KEY").ok() else {
            return Ok(None);
        };
        let bytes = parse_key_bytes(raw.trim())?;
        let cipher = Aes256Gcm::new_from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("TOKEN_ENCRYPTION_KEY has invalid length"))?;
        Ok(Some(Self {
            cipher: Arc::new(cipher),
        }))
    }

    /// Encrypt `plaintext` and return a base64 string suitable for storage
    /// in a TEXT column.
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::try_from(&nonce_bytes[..]).expect("nonce length is fixed");
        let ct = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("aes-gcm encrypt: {e}"))?;
        let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
        out.push(VERSION_AES_GCM_V1);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(B64.encode(out))
    }

    /// Decrypt a value produced by `encrypt`. Returns an error on tag
    /// mismatch (= corruption or wrong key) or unknown version byte.
    pub fn decrypt(&self, blob: &str) -> Result<String> {
        let raw = B64
            .decode(blob.trim().as_bytes())
            .context("decrypt: not valid base64")?;
        if raw.len() < 1 + NONCE_LEN + 16 {
            bail!("decrypt: payload too short");
        }
        match raw[0] {
            VERSION_AES_GCM_V1 => {
                let nonce =
                    Nonce::try_from(&raw[1..1 + NONCE_LEN]).expect("nonce length was checked");
                let ct = &raw[1 + NONCE_LEN..];
                let pt = self
                    .cipher
                    .decrypt(&nonce, ct)
                    .map_err(|e| anyhow::anyhow!("aes-gcm decrypt: {e}"))?;
                String::from_utf8(pt).context("decrypt: not valid utf-8")
            }
            other => bail!("decrypt: unknown ciphertext version {other:#04x}"),
        }
    }
}

/// Decode `TOKEN_ENCRYPTION_KEY` as either base64 (standard or url-safe)
/// or hex, returning the 32 raw bytes. Failure mode is a clear error
/// message that hints at both supported formats.
fn parse_key_bytes(raw: &str) -> Result<[u8; KEY_LEN]> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    // Try standard base64 first (matches our docs default).
    if let Ok(b) = B64.decode(raw.as_bytes())
        && b.len() == KEY_LEN
    {
        return Ok(b.try_into().expect("len-checked"));
    }
    // Try url-safe base64 (some token generators emit this).
    if let Ok(b) = URL_SAFE_NO_PAD.decode(raw.as_bytes())
        && b.len() == KEY_LEN
    {
        return Ok(b.try_into().expect("len-checked"));
    }
    // Try hex (e.g. `openssl rand -hex 32`).
    if raw.len() == KEY_LEN * 2
        && let Ok(b) = hex::decode(raw)
        && b.len() == KEY_LEN
    {
        return Ok(b.try_into().expect("len-checked"));
    }
    bail!(
        "TOKEN_ENCRYPTION_KEY: expected 32 raw bytes encoded as base64 \
         (44 chars from `openssl rand -base64 32`) or hex (64 chars from \
         `openssl rand -hex 32`); decoded length was wrong"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Crypto {
        // 32 zero bytes — fine for testing.
        let bytes = [0u8; KEY_LEN];
        Crypto {
            cipher: Arc::new(Aes256Gcm::new_from_slice(&bytes).unwrap()),
        }
    }

    #[test]
    fn roundtrip_preserves_plaintext() {
        let c = fixture();
        let blob = c.encrypt("ghu_xxxxxxxxxxxxxxxxxxxx").unwrap();
        assert!(blob.len() > 24);
        let pt = c.decrypt(&blob).unwrap();
        assert_eq!(pt, "ghu_xxxxxxxxxxxxxxxxxxxx");
    }

    #[test]
    fn each_encrypt_uses_fresh_nonce() {
        let c = fixture();
        let a = c.encrypt("same plaintext").unwrap();
        let b = c.encrypt("same plaintext").unwrap();
        assert_ne!(a, b, "ciphertexts must differ — random nonce required");
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let c = fixture();
        let mut blob = c.encrypt("hello").unwrap();
        // Flip a byte deep in the ciphertext.
        let mut raw = B64.decode(blob.as_bytes()).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        blob = B64.encode(&raw);
        assert!(c.decrypt(&blob).is_err());
    }

    #[test]
    fn parse_key_accepts_hex() {
        let hex = "0".repeat(KEY_LEN * 2);
        let bytes = parse_key_bytes(&hex).unwrap();
        assert_eq!(bytes, [0u8; KEY_LEN]);
    }

    #[test]
    fn parse_key_accepts_base64() {
        let b64 = B64.encode([0u8; KEY_LEN]);
        let bytes = parse_key_bytes(&b64).unwrap();
        assert_eq!(bytes, [0u8; KEY_LEN]);
    }

    #[test]
    fn parse_key_rejects_short_input() {
        let too_short = B64.encode([0u8; 24]);
        let err = parse_key_bytes(&too_short).unwrap_err();
        assert!(err.to_string().contains("32 raw bytes"));
    }

    #[test]
    fn unknown_version_rejected() {
        let c = fixture();
        let blob = c.encrypt("x").unwrap();
        let mut raw = B64.decode(blob.as_bytes()).unwrap();
        raw[0] = 0xff;
        let blob = B64.encode(&raw);
        let err = c.decrypt(&blob).unwrap_err();
        assert!(err.to_string().contains("unknown ciphertext version"));
    }
}
