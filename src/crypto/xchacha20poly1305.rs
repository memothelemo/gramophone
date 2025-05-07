use super::{AEAD_KEY_LEN, Aead, AeadError, AeadErrorType};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305 as Cipher, XNonce, aead::AeadInPlace};
use std::fmt::Debug;

const NONCE_LEN: usize = 24;

pub struct XChaCha20Poly1035 {
    cipher: Cipher,
}

impl Debug for XChaCha20Poly1035 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XChaCha20Poly1035").finish_non_exhaustive()
    }
}

impl XChaCha20Poly1035 {
    // Clippy: It is already assumed that key.len() == AEAD_KEY_LEN
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new(key: &[u8]) -> Option<Self> {
        (key.len() == AEAD_KEY_LEN).then(|| {
            let key = key
                .try_into()
                .expect("key should have the size of AEAD_KEY_LEN");

            Self::new_sized(key)
        })
    }

    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new_sized(key: &[u8; AEAD_KEY_LEN]) -> Self {
        Self {
            cipher: Cipher::new_from_slice(key).expect("key should have the size of AEAD_KEY_LEN"),
        }
    }
}

impl Aead for XChaCha20Poly1035 {
    #[must_use]
    fn mode(&self) -> super::EncryptMode {
        super::EncryptMode::XChaCha20Poly1305
    }

    fn encrypt(&self, nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AeadError> {
        if nonce.len() != NONCE_LEN {
            return Err(AeadError {
                kind: AeadErrorType::InvalidNonceLength {
                    expected: NONCE_LEN,
                },
            });
        }

        let mut buffer = plaintext.to_vec();
        let nonce = XNonce::from_slice(nonce);
        self.cipher
            .encrypt_in_place(nonce, aad, &mut buffer)
            .map_err(|_| AeadError {
                kind: AeadErrorType::Unspecified,
            })?;

        Ok(buffer)
    }

    fn decrypt(&self, nonce: &[u8], aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, AeadError> {
        if nonce.len() != NONCE_LEN {
            return Err(AeadError {
                kind: AeadErrorType::InvalidNonceLength {
                    expected: NONCE_LEN,
                },
            });
        }

        let mut buffer = ciphertext.to_vec();
        let nonce = XNonce::from_slice(nonce);
        self.cipher
            .decrypt_in_place(nonce, aad, &mut buffer)
            .map_err(|_| AeadError {
                kind: AeadErrorType::Unspecified,
            })?;

        Ok(buffer)
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::{Aead, XChaCha20Poly1035};

    const SECRET_KEY: &str = "fb5e9f96f291742023f321c7f4f967a2d90f4e7dadb2d9bd66774ad3ebb3ac5d";
    const NONCE: &str = "013bf700df8ad5e1fd9cbbefcb65e060d4e0b6e8a40865ec";

    // Generated with node.js (encoded with hex)
    //
    // const secretKey = Buffer.from("fb5e9f96f291742023f321c7f4f967a2d90f4e7dadb2d9bd66774ad3ebb3ac5d", "hex")
    // const nonce = Buffer.from("013bf700df8ad5e1fd9cbbefcb65e060d4e0b6e8a40865ec", "hex")
    //
    // const chacha = noble.xchacha20poly1305(Uint8Array.from(secretKey), Uint8Array.from(nonce), Uint8Array.from(Buffer.from("", "hex")))
    // Buffer.from(chacha.encrypt(Uint8Array.from(Buffer.from("Hello, World!", "utf-8")))).toString("hex")
    const CIPHERTEXT: &str = "cc1f35d59684ffed96687535cafaba8431ec674942f4c67edb39bca372";

    #[test]
    fn test_decrypt() {
        let key = hex::decode(SECRET_KEY).unwrap();
        let nonce = hex::decode(NONCE).unwrap();
        let ciphertext = hex::decode(CIPHERTEXT).unwrap();

        let chacha = XChaCha20Poly1035::new(&key).unwrap();
        let plaintext = chacha.decrypt(&nonce, &[], &ciphertext).unwrap();

        assert_eq!(plaintext, b"Hello, World!");
    }

    #[test]
    fn test_encrypt() {
        let key = hex::decode(SECRET_KEY).unwrap();
        let nonce = hex::decode(NONCE).unwrap();

        let chacha = XChaCha20Poly1035::new(&key).unwrap();
        let cipher = chacha
            .encrypt(&nonce, &[], b"Hello, World!")
            .map(hex::encode)
            .unwrap();

        assert_eq!(cipher, CIPHERTEXT);
    }
}
