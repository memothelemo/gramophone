//! Platform-agnostic crypto module wrapper that handles encryption for
//! both crypto providers, ring and aws-lc-rs.
use std::fmt::{Debug, Display};

// We're using ring if both aws-lc-rs and ring are configured.
#[cfg(any(
    all(feature = "ring", not(feature = "aws_lc_rs")),
    all(feature = "aws_lc_rs", feature = "ring")
))]
extern crate ring;

// Since aws-lc-rs claims to be ring-compatible so we don't need to isolate
// each of their implementation in designated modules.
#[cfg(all(feature = "aws_lc_rs", not(feature = "ring")))]
extern crate aws_lc_rs as ring;

pub mod discord;
pub use self::discord::EncryptMode;

pub mod aes256gcm;
pub mod xchacha20poly1305;

pub use self::aes256gcm::Aes256Gcm;
pub use self::xchacha20poly1305::XChaCha20Poly1035;

/// Platform-agnostic trait for encrypting and decrypting data using
/// the `AES-256-GCM` or `XChaCha20Poly1305` encryption algorithm,
/// independent of any specific crypto provider implementation.
pub trait Aead: Debug + Sync + Send {
    fn mode(&self) -> EncryptMode;
    fn encrypt(&self, nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AeadError>;
    fn decrypt(&self, nonce: &[u8], aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, AeadError>;
}

/// Secret key size to encrypt/decrypt voice packets
pub const AEAD_KEY_LEN: usize = 32;

pub struct AeadError {
    pub(crate) kind: AeadErrorType,
}

impl AeadError {
    #[must_use]
    pub fn kind(&self) -> &AeadErrorType {
        &self.kind
    }
}

impl Debug for AeadError {
    #[cfg(not(test))]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeadError").finish_non_exhaustive()
    }

    #[cfg(test)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeadError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Display for AeadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // This is on purpose so the attacker cannot decipher
        // the cause of the error.
        f.write_str("aead error")
    }
}

impl std::error::Error for AeadError {}

#[cfg_attr(test, derive(Debug))]
#[non_exhaustive]
pub enum AeadErrorType {
    /// General AEAD error. Nothing too specific.
    Unspecified,

    /// Invalid nonce length.
    InvalidNonceLength { expected: usize },
}
