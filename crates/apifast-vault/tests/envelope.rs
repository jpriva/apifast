//! The `enc:v1:` envelope: the cipher matches the published test vector,
//! the format never changes, and a value opens only with its key and for
//! the variable it was sealed for.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_vault::{Aad, Key, NONCE_LEN, PREFIX, VaultError, open, seal, seal_with_nonce};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn token() -> Aad {
    Aad::variable("col_1", "prod", "token")
}

/// `AEAD_XChaCha20_Poly1305`, appendix A.3.1 of `draft-irtf-cfrg-xchacha-03`.
#[test]
fn the_cipher_matches_the_published_test_vector() {
    let key = Key::from_bytes(
        hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f")
            .try_into()
            .unwrap(),
    );
    let nonce: [u8; NONCE_LEN] = hex("404142434445464748494a4b4c4d4e4f5051525354555657")
        .try_into()
        .unwrap();
    let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let aad = Aad::raw(hex("50515253c0c1c2c3c4c5c6c7"));
    let ciphertext = hex(concat!(
        "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb",
        "731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452",
        "2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9",
        "21f9664c97637da9768812f615c68b13b52e"
    ));
    let tag = hex("c0875924c1c7987947deafd8780acf49");

    let envelope = seal_with_nonce(&key, &nonce, plaintext, &aad).unwrap();
    let bytes = URL_SAFE_NO_PAD
        .decode(envelope.strip_prefix(PREFIX).unwrap())
        .unwrap();
    assert_eq!(bytes[..NONCE_LEN], nonce);
    assert_eq!(bytes[NONCE_LEN..bytes.len() - 16], ciphertext);
    assert_eq!(bytes[bytes.len() - 16..], tag);
    assert_eq!(&open(&key, &envelope, &aad).unwrap()[..], plaintext);
}

/// apifast's own format, with its associated data: if a dependency update
/// changed it, values already written would no longer open.
#[test]
fn the_envelope_format_never_changes() {
    let key = Key::from_bytes([7; 32]);
    let envelope = seal_with_nonce(&key, &[9; NONCE_LEN], b"s3cret", &token()).unwrap();
    assert_eq!(envelope, FIXED_ENVELOPE);
    assert_eq!(
        &open(&key, FIXED_ENVELOPE, &token()).unwrap()[..],
        b"s3cret"
    );
}

const FIXED_ENVELOPE: &str =
    "enc:v1:CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJzUGB-BnefjwsRiV17pCz9f46Cb_OpA";

#[test]
fn a_value_opens_with_its_key_and_nowhere_else() {
    let key = Key::generate().unwrap();
    let envelope = seal(&key, b"s3cret", &token()).unwrap();
    assert!(envelope.starts_with(PREFIX));
    assert_eq!(&open(&key, &envelope, &token()).unwrap()[..], b"s3cret");

    let other_key = Key::generate().unwrap();
    assert_eq!(
        open(&other_key, &envelope, &token()),
        Err(VaultError::Rejected)
    );
    for elsewhere in [
        Aad::variable("col_1", "prod", "password"),
        Aad::variable("col_1", "staging", "token"),
        Aad::variable("col_2", "prod", "token"),
    ] {
        assert_eq!(open(&key, &envelope, &elsewhere), Err(VaultError::Rejected));
    }
}

#[test]
fn a_changed_value_does_not_open() {
    let key = Key::generate().unwrap();
    let envelope = seal(&key, b"s3cret", &token()).unwrap();
    let mut bytes = URL_SAFE_NO_PAD
        .decode(envelope.strip_prefix(PREFIX).unwrap())
        .unwrap();
    for at in [0, NONCE_LEN, bytes.len() - 1] {
        bytes[at] ^= 1;
        let changed = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&bytes));
        assert_eq!(
            open(&key, &changed, &token()),
            Err(VaultError::Rejected),
            "byte {at}"
        );
        bytes[at] ^= 1;
    }
}

#[test]
fn texts_that_are_not_envelopes_are_told_apart() {
    let key = Key::generate().unwrap();
    for text in ["s3cret", "enc:v1:", "enc:v1:!!!", "enc:v1:AAAA"] {
        assert_eq!(
            open(&key, text, &token()),
            Err(VaultError::NotAnEnvelope),
            "{text}"
        );
    }
    assert_eq!(
        open(&key, "enc:v2:AAAA", &token()),
        Err(VaultError::UnknownVersion("v2".into()))
    );
}

#[test]
fn every_seal_uses_a_new_nonce_and_keys_never_print() {
    let key = Key::generate().unwrap();
    let a = seal(&key, b"same", &token()).unwrap();
    let b = seal(&key, b"same", &token()).unwrap();
    assert_ne!(a, b);
    assert_eq!(format!("{key:?}"), "Key(••••••)");
}
