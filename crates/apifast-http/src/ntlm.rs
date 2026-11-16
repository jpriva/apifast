//! NTLM authentication, version 2 ([MS-NLMP]): the Negotiate message, then
//! the Authenticate message that answers the server's Challenge.
//!
//! NTLM authenticates the connection, not each request, so the three
//! messages must use one connection; the transport sees to that.

use hmac::{Hmac, KeyInit, Mac};
use md4::{Digest as _, Md4};
use md5::Md5;

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";

const NEGOTIATE_UNICODE: u32 = 0x0000_0001;
const NEGOTIATE_OEM: u32 = 0x0000_0002;
const REQUEST_TARGET: u32 = 0x0000_0004;
const NEGOTIATE_NTLM: u32 = 0x0000_0200;
const NEGOTIATE_ALWAYS_SIGN: u32 = 0x0000_8000;
const NEGOTIATE_EXTENDED_SESSIONSECURITY: u32 = 0x0008_0000;
const NEGOTIATE_TARGET_INFO: u32 = 0x0080_0000;
const NEGOTIATE_128: u32 = 0x2000_0000;
const NEGOTIATE_56: u32 = 0x8000_0000;

const FLAGS: u32 = NEGOTIATE_UNICODE
    | NEGOTIATE_OEM
    | REQUEST_TARGET
    | NEGOTIATE_NTLM
    | NEGOTIATE_ALWAYS_SIGN
    | NEGOTIATE_EXTENDED_SESSIONSECURITY
    | NEGOTIATE_TARGET_INFO
    | NEGOTIATE_128
    | NEGOTIATE_56;

/// Size of the Authenticate message's header: signature, type, six
/// security buffers (8 bytes each) and flags.
const HEADER: usize = 8 + 4 + 6 * 8 + 4;

/// The id of the timestamp among the target info's pairs.
const AV_TIMESTAMP: u16 = 7;

/// The Negotiate message (type 1).
pub fn negotiate() -> Vec<u8> {
    let mut message = Vec::with_capacity(32);
    message.extend_from_slice(SIGNATURE);
    message.extend_from_slice(&1u32.to_le_bytes());
    message.extend_from_slice(&FLAGS.to_le_bytes());
    // No domain and no workstation.
    message.extend_from_slice(&[0; 16]);
    message
}

/// What the server's Challenge message (type 2) says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub flags: u32,
    pub nonce: [u8; 8],
    pub target_info: Vec<u8>,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

impl Challenge {
    /// Reads a Challenge message; `None` if it is not one.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.get(..8)? != SIGNATURE || u32_at(bytes, 8)? != 2 {
            return None;
        }
        let flags = u32_at(bytes, 20)?;
        let nonce = bytes.get(24..32)?.try_into().ok()?;
        let target_info = match (u16_at(bytes, 40), u32_at(bytes, 44)) {
            (Some(len), Some(offset)) if len > 0 => {
                let start = usize::try_from(offset).ok()?;
                bytes.get(start..start + usize::from(len))?.to_vec()
            }
            _ => Vec::new(),
        };
        Some(Self {
            flags,
            nonce,
            target_info,
        })
    }

    /// The server's time among the target info's pairs, if it sent one.
    fn timestamp(&self) -> Option<[u8; 8]> {
        let info = &self.target_info;
        let mut at = 0;
        while let (Some(id), Some(len)) = (u16_at(info, at), u16_at(info, at + 2)) {
            let value = info.get(at + 4..at + 4 + usize::from(len))?;
            match id {
                0 => return None,
                AV_TIMESTAMP => return value.try_into().ok(),
                _ => at += 4 + usize::from(len),
            }
        }
        None
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    #[expect(clippy::expect_used, reason = "HMAC accepts keys of any length")]
    let mut mac = Hmac::<Md5>::new_from_slice(key).expect("any key length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// `NTOWFv2`: the key of the responses, from the password, user and domain.
fn response_key(username: &str, password: &str, domain: &str) -> [u8; 16] {
    let nt_hash: [u8; 16] = Md4::digest(utf16(password)).into();
    let identity = utf16(&format!("{}{domain}", username.to_uppercase()));
    hmac_md5(&nt_hash, &[&identity])
}

/// The credentials and what varies between answers.
pub struct Credentials<'a> {
    pub username: &'a str,
    pub password: &'a str,
    pub domain: &'a str,
    pub workstation: &'a str,
    /// Eight random bytes.
    pub client_challenge: [u8; 8],
    /// Now, in 100 ns since 1601, used when the server sends no time.
    pub now: u64,
}

/// The `NTLMv2` and `LMv2` responses to `challenge`.
fn responses(challenge: &Challenge, c: &Credentials<'_>) -> (Vec<u8>, Vec<u8>) {
    let key = response_key(c.username, c.password, c.domain);
    let time = challenge.timestamp().unwrap_or(c.now.to_le_bytes());
    let mut temp = vec![1, 1, 0, 0, 0, 0, 0, 0];
    temp.extend_from_slice(&time);
    temp.extend_from_slice(&c.client_challenge);
    temp.extend_from_slice(&[0; 4]);
    temp.extend_from_slice(&challenge.target_info);
    temp.extend_from_slice(&[0; 4]);
    let proof = hmac_md5(&key, &[&challenge.nonce, &temp]);
    let mut nt = proof.to_vec();
    nt.extend_from_slice(&temp);
    let mut lm = hmac_md5(&key, &[&challenge.nonce, &c.client_challenge]).to_vec();
    lm.extend_from_slice(&c.client_challenge);
    (nt, lm)
}

/// The Authenticate message (type 3) answering `challenge`.
pub fn authenticate(challenge: &Challenge, credentials: &Credentials<'_>) -> Vec<u8> {
    let (nt, lm) = responses(challenge, credentials);
    let fields: [Vec<u8>; 5] = [
        lm,
        nt,
        utf16(credentials.domain),
        utf16(credentials.username),
        utf16(credentials.workstation),
    ];
    let mut message = Vec::new();
    message.extend_from_slice(SIGNATURE);
    message.extend_from_slice(&3u32.to_le_bytes());
    let mut offset = HEADER;
    let mut buffer = |message: &mut Vec<u8>, len: usize| {
        // Messages are far below 64 KiB.
        #[expect(clippy::cast_possible_truncation, reason = "fields are small")]
        let (len16, at) = (len as u16, offset as u32);
        message.extend_from_slice(&len16.to_le_bytes());
        message.extend_from_slice(&len16.to_le_bytes());
        message.extend_from_slice(&at.to_le_bytes());
        offset += len;
    };
    for field in &fields {
        buffer(&mut message, field.len());
    }
    // No session key.
    buffer(&mut message, 0);
    let flags = FLAGS & (challenge.flags | !NEGOTIATE_TARGET_INFO);
    message.extend_from_slice(&flags.to_le_bytes());
    for field in &fields {
        message.extend_from_slice(field);
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let text: String = text.split_whitespace().collect();
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    /// The `NTLMv2` example of \[MS-NLMP\] section 4.2.4.
    fn example() -> (Challenge, Credentials<'static>) {
        let target_info = hex("02 00 0c 00 44 00 6f 00 6d 00 61 00 69 00 6e 00
             01 00 0c 00 53 00 65 00 72 00 76 00 65 00 72 00
             00 00 00 00");
        (
            Challenge {
                flags: FLAGS,
                nonce: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef],
                target_info,
            },
            Credentials {
                username: "User",
                password: "Password",
                domain: "Domain",
                workstation: "COMPUTER",
                client_challenge: [0xaa; 8],
                now: 0,
            },
        )
    }

    #[test]
    fn keys_and_responses_match_the_specification() {
        let (challenge, credentials) = example();
        let nt_hash: [u8; 16] = Md4::digest(utf16("Password")).into();
        assert_eq!(nt_hash.to_vec(), hex("a4f49c406510bdcab6824ee7c30fd852"));
        assert_eq!(
            response_key("User", "Password", "Domain").to_vec(),
            hex("0c868a403bfd7a93a3001ef22ef02e3f")
        );
        let (nt, lm) = responses(&challenge, &credentials);
        assert_eq!(nt[..16], hex("68cd0ab851e51c96aabc927bebef6a1c")[..]);
        assert_eq!(lm, hex("86c35097ac9cec102554764a57cccc19 aaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn messages_parse_and_point_at_their_fields() {
        let (challenge, credentials) = example();
        let mut type2 = Vec::new();
        type2.extend_from_slice(SIGNATURE);
        type2.extend_from_slice(&2u32.to_le_bytes());
        type2.extend_from_slice(&[0; 8]); // target name
        type2.extend_from_slice(&FLAGS.to_le_bytes());
        type2.extend_from_slice(&challenge.nonce);
        type2.extend_from_slice(&[0; 8]); // reserved
        #[expect(clippy::cast_possible_truncation, reason = "a small test buffer")]
        let len = challenge.target_info.len() as u16;
        type2.extend_from_slice(&len.to_le_bytes());
        type2.extend_from_slice(&len.to_le_bytes());
        type2.extend_from_slice(&48u32.to_le_bytes());
        type2.extend_from_slice(&challenge.target_info);
        assert_eq!(Challenge::parse(&type2), Some(challenge.clone()));
        assert_eq!(Challenge::parse(b"NTLMSSP\0\x01\0\0\0"), None);

        let message = authenticate(&challenge, &credentials);
        assert_eq!(&message[..8], SIGNATURE);
        assert_eq!(u32_at(&message, 8), Some(3));
        // The user name is where its buffer says, in UTF-16.
        let len = usize::from(u16_at(&message, 36).unwrap());
        let at = usize::try_from(u32_at(&message, 40).unwrap()).unwrap();
        assert_eq!(&message[at..at + len], utf16("User").as_slice());
        assert_eq!(negotiate().len(), 32);
    }

    #[test]
    fn the_server_time_is_used_when_sent() {
        let (mut challenge, credentials) = example();
        let mut info = vec![7, 0, 8, 0];
        info.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        info.extend_from_slice(&[0, 0, 0, 0]);
        challenge.target_info = info;
        assert_eq!(challenge.timestamp(), Some([1, 2, 3, 4, 5, 6, 7, 8]));
        let (nt, _) = responses(&challenge, &credentials);
        assert_eq!(&nt[24..32], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
