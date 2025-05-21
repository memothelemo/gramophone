use std::fmt::Display;
use std::str::FromStr;

use super::{Aead, Aes256Gcm, XChaCha20Poly1035};
use gramophone_types::RTP_KEY_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptMode {
    /// AEAD `AES256-GCM` (RTP Size) (Preferred)
    Aes256Gcm,
    /// AEAD `XChaCha20` Poly1305 (RTP Size) (Required)
    XChaCha20Poly1305,
}

impl EncryptMode {
    /// Generates an [AEAD encryptor] based on the mode from a secret key.
    ///
    /// [AEAD encryptor]: Aead
    #[must_use]
    pub fn encryptor(&self, key: &[u8; RTP_KEY_LEN]) -> Box<dyn Aead> {
        match self {
            Self::Aes256Gcm => Box::new(Aes256Gcm::new_sized(key)),
            Self::XChaCha20Poly1305 => Box::new(XChaCha20Poly1035::new_sized(key)),
        }
    }
}

impl EncryptMode {
    /// Returns the best encryption mode based on the available modes
    /// given from the [ready payload].
    ///
    /// [ready payload]: gramophone_types::payload::incoming::Ready
    #[must_use]
    pub fn negotiate<T: AsRef<str>>(available_modes: &[T]) -> Option<Self> {
        let mut best = None;

        for mode in available_modes {
            let Ok(mode) = EncryptMode::from_str(mode.as_ref()) else {
                // unsupported mode
                continue;
            };

            let priority = mode.priority();
            let accept = match best {
                None => true,
                Some((_, score)) if priority > score => true,
                _ => false,
            };

            if accept {
                best = Some((mode, priority));
            }
        }

        best.map(|v| v.0)
    }

    /// Gets the required size of a nonce for a particular mode.
    #[must_use]
    pub const fn nonce_size(&self) -> usize {
        match self {
            Self::Aes256Gcm => 12,
            Self::XChaCha20Poly1305 => 24,
        }
    }
}

impl EncryptMode {
    /// Returns the name of a mode as it will appear during negotiation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Aes256Gcm => "aead_aes256_gcm_rtpsize",
            Self::XChaCha20Poly1305 => "aead_xchacha20_poly1305_rtpsize",
        }
    }
}

impl EncryptMode {
    /// Returns a local priority score for a given [mode].
    ///
    /// Higher values are more preferred.
    ///
    /// [mode]: EncryptMode
    #[must_use]
    const fn priority(self) -> u64 {
        match self {
            Self::Aes256Gcm => 1,
            Self::XChaCha20Poly1305 => 0,
        }
    }
}

impl Display for EncryptMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownEncryptMode {
    mode: String,
}

impl UnknownEncryptMode {
    #[must_use]
    pub fn mode(&self) -> &str {
        &self.mode
    }
}

impl std::fmt::Display for UnknownEncryptMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unknown encrypt mode: ")?;
        std::fmt::Debug::fmt(&self.mode, f)
    }
}

impl std::error::Error for UnknownEncryptMode {}

impl std::str::FromStr for EncryptMode {
    type Err = UnknownEncryptMode;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "aead_aes256_gcm_rtpsize" => Ok(Self::Aes256Gcm),
            "aead_xchacha20_poly1305_rtpsize" => Ok(Self::XChaCha20Poly1305),
            _ => Err(UnknownEncryptMode {
                mode: s.to_string(),
            }),
        }
    }
}
