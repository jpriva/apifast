//! Encrypted secrets of apifast (see `docs/06-boveda-de-secretos.md`).
//!
//! Every encrypted value is an envelope:
//!
//! ```text
//! enc:v1:<base64url without padding of ( 24-byte nonce | ciphertext | 16-byte tag )>
//! ```
//!
//! sealed with XChaCha20-Poly1305 under a [`Key`], with [`Aad`] that binds
//! the value to its collection, environment and variable: a value copied
//! anywhere else does not open.

mod collection;
mod keys;
mod local;

pub use collection::{CollectionKeyStore, check_challenge, new_challenge};

pub use keys::{
    KEY_FILE, KEYRING_ENTRY, KEYRING_SERVICE, KeyBackend, KeyFile, KeyLocation, KeyStoreError,
    SystemKeyring, existing_master_key, master_key,
};
pub use local::{LocalVault, SecretEntry, VaultSecrets};

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// What starts every envelope of this version.
pub const PREFIX: &str = "enc:v1:";
/// Bytes of a key.
pub const KEY_LEN: usize = 32;
/// Bytes of a nonce.
pub const NONCE_LEN: usize = 24;
/// Bytes of the authentication tag.
pub const TAG_LEN: usize = 16;

/// Why a value could not be sealed or opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VaultError {
    /// The text is not an envelope apifast can read.
    #[error("not an encrypted value")]
    NotAnEnvelope,
    /// The envelope is of a version this apifast does not know.
    #[error("encrypted with a newer version (`{0}`)")]
    UnknownVersion(String),
    /// The key is wrong, or the value was changed or moved to another
    /// variable. Authenticated encryption cannot tell which.
    #[error("cannot be decrypted: wrong key, or the value was changed or moved")]
    Rejected,
    /// The system gave no randomness.
    #[error("no randomness available: {0}")]
    Randomness(String),
}

/// A 32-byte key. Wiped from memory when dropped; never printed.
#[derive(Clone)]
pub struct Key([u8; KEY_LEN]);

impl Drop for Key {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl ZeroizeOnDrop for Key {}

impl Key {
    /// A new random key.
    ///
    /// # Errors
    ///
    /// If the system gives no randomness.
    pub fn generate() -> Result<Self, VaultError> {
        let mut bytes = [0; KEY_LEN];
        random(&mut bytes)?;
        Ok(Self(bytes))
    }

    /// A key from its bytes, such as read from the keyring.
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// Its bytes, to store it.
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(&self.0.into())
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(••••••)")
    }
}

/// The associated data of a value: what it belongs to. Opening it with
/// other associated data fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aad(Vec<u8>);

impl Aad {
    /// For variable `variable` of environment `environment` in collection
    /// `collection` (its stable id).
    pub fn variable(collection: &str, environment: &str, variable: &str) -> Self {
        Self(format!("apifast/v1|{collection}|{environment}|{variable}").into_bytes())
    }

    /// For something that is not a variable, such as the challenge of a
    /// collection: `kind` says what it is.
    pub fn other(collection: &str, kind: &str) -> Self {
        Self(format!("apifast/v1|{collection}|{kind}").into_bytes())
    }

    /// Any bytes. Only for test vectors from elsewhere.
    #[doc(hidden)]
    pub fn raw(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The bytes that are authenticated.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// `plaintext` sealed under `key` for `aad`, with a fresh random nonce.
///
/// # Errors
///
/// If the system gives no randomness.
pub fn seal(key: &Key, plaintext: &[u8], aad: &Aad) -> Result<String, VaultError> {
    let mut nonce = [0; NONCE_LEN];
    random(&mut nonce)?;
    seal_with_nonce(key, &nonce, plaintext, aad)
}

/// [`seal`] with a given nonce. Only for test vectors: a nonce must never be
/// used twice with one key.
#[doc(hidden)]
pub fn seal_with_nonce(
    key: &Key,
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
    aad: &Aad,
) -> Result<String, VaultError> {
    let sealed = key
        .cipher()
        .encrypt(
            &XNonce::from(*nonce),
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        // Only fails for inputs larger than the cipher allows (256 GiB).
        .map_err(|_| VaultError::Rejected)?;
    let mut bytes = Vec::with_capacity(NONCE_LEN + sealed.len());
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(&sealed);
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&bytes)))
}

/// The plaintext of `envelope`, sealed under `key` for `aad`. It is wiped
/// from memory when dropped.
///
/// # Errors
///
/// [`VaultError::NotAnEnvelope`] or [`VaultError::UnknownVersion`] if the
/// text cannot be read; [`VaultError::Rejected`] if the key or the
/// associated data are wrong, or the value was changed.
pub fn open(key: &Key, envelope: &str, aad: &Aad) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let Some(encoded) = envelope.strip_prefix(PREFIX) else {
        return Err(match envelope.strip_prefix("enc:") {
            Some(rest) => {
                VaultError::UnknownVersion(rest.split(':').next().unwrap_or_default().to_owned())
            }
            None => VaultError::NotAnEnvelope,
        });
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| VaultError::NotAnEnvelope)?;
    if bytes.len() < NONCE_LEN + TAG_LEN {
        return Err(VaultError::NotAnEnvelope);
    }
    let (nonce, sealed) = bytes.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| VaultError::NotAnEnvelope)?;
    key.cipher()
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: sealed,
                aad: aad.as_bytes(),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| VaultError::Rejected)
}

fn random(buffer: &mut [u8]) -> Result<(), VaultError> {
    getrandom::fill(buffer).map_err(|e| VaultError::Randomness(e.to_string()))
}
