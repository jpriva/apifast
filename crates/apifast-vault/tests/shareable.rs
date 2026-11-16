//! The collection's key as people pass it: `AFK-` and 9 groups of 6.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_vault::{Key, ShareableKeyError, parse_shareable_key, shareable_key};

#[test]
fn keys_are_written_in_nine_groups_and_read_back() {
    for _ in 0..200 {
        let key = Key::generate().unwrap();
        let text = shareable_key(&key);
        let groups: Vec<&str> = text.split('-').collect();
        assert_eq!(groups[0], "AFK");
        assert_eq!(groups.len(), 10, "{text}");
        assert!(groups[1..].iter().all(|g| g.len() == 6), "{text}");
        assert!(
            !text[4..].contains(['I', 'L', 'O', 'U']),
            "no ambiguous characters: {text}"
        );
        assert_eq!(
            parse_shareable_key(&text).unwrap().as_bytes(),
            key.as_bytes()
        );
    }
}

#[test]
fn reading_is_lenient_with_case_dashes_spaces_and_lookalikes() {
    let key = Key::from_bytes([0x01; 32]);
    let text = shareable_key(&key);
    let relaxed = text
        .to_lowercase()
        .replace('-', " ")
        .replace('0', "o")
        .replace('1', "l");
    assert_eq!(
        parse_shareable_key(&relaxed).unwrap().as_bytes(),
        key.as_bytes()
    );
    let without_prefix = text.trim_start_matches("AFK-");
    assert_eq!(
        parse_shareable_key(without_prefix).unwrap().as_bytes(),
        key.as_bytes()
    );
}

#[test]
fn a_typo_is_caught_before_the_key_is_tried() {
    let key = Key::from_bytes([0x5a; 32]);
    let text = shareable_key(&key);
    let symbols: Vec<char> = text.chars().collect();
    // Change each character of the key and check parts, one at a time.
    for at in (4..symbols.len()).filter(|&i| symbols[i] != '-') {
        let mut typo = symbols.clone();
        typo[at] = if symbols[at] == 'X' { 'Y' } else { 'X' };
        let typo: String = typo.into_iter().collect();
        assert!(parse_shareable_key(&typo).is_err(), "typo at {at}: {typo}");
    }
}

#[test]
fn what_is_not_a_key_is_told_apart_from_a_typo() {
    assert_eq!(
        parse_shareable_key("AFK-123").unwrap_err(),
        ShareableKeyError::Format
    );
    assert_eq!(
        parse_shareable_key("").unwrap_err(),
        ShareableKeyError::Format
    );
    let text = shareable_key(&Key::from_bytes([3; 32]));
    let with_u = format!("{}U{}", &text[..10], &text[11..]);
    assert_eq!(
        parse_shareable_key(&with_u).unwrap_err(),
        ShareableKeyError::Format
    );
}
