//! Dynamic variables (`{{$name}}`), generated on every send.
//!
//! Time and randomness come from the [`Host`](crate::Host), so this module
//! stays pure and testable.

use std::fmt::Write as _;

use crate::Host;

/// Writes the value of dynamic variable `name` into `out`. Returns `false` if
/// the variable is unknown.
pub(crate) fn generate(name: &str, host: &mut dyn Host, out: &mut String) -> bool {
    match name {
        "randomUUID" => uuid_v4(host.random_u64(), host.random_u64(), out),
        "timestamp" => push_display(out, host.unix_millis().div_euclid(1000)),
        "isoTimestamp" => iso_8601(host.unix_millis(), out),
        "randomInt" => push_display(out, host.random_u64() % 1001),
        _ => return false,
    }
    true
}

fn push_display(out: &mut String, value: impl std::fmt::Display) {
    // Writing to a String cannot fail.
    let _ = write!(out, "{value}");
}

/// A version 4 UUID built from 128 random bits.
fn uuid_v4(high: u64, low: u64, out: &mut String) {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&high.to_be_bytes());
    bytes[8..].copy_from_slice(&low.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        push_display(out, format_args!("{byte:02x}"));
    }
}

/// UTC date and time in ISO 8601 with milliseconds, such as
/// `2023-11-14T22:13:20.123Z`.
fn iso_8601(unix_millis: i64, out: &mut String) {
    let secs = unix_millis.div_euclid(1000);
    let millis = unix_millis.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    push_display(
        out,
        format_args!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
            day_secs / 3600,
            day_secs % 3600 / 60,
            day_secs % 60
        ),
    );
}

/// Converts days since 1970-01-01 to a (year, month, day) civil date.
/// Howard Hinnant's algorithm: <https://howardhinnant.github.io/date_algorithms.html>.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso(millis: i64) -> String {
        let mut out = String::new();
        iso_8601(millis, &mut out);
        out
    }

    #[test]
    fn iso_timestamps_match_known_dates() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso(-1), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn uuids_have_version_4_and_the_rfc_variant() {
        let mut out = String::new();
        uuid_v4(u64::MAX, u64::MAX, &mut out);
        assert_eq!(out, "ffffffff-ffff-4fff-bfff-ffffffffffff");
        out.clear();
        uuid_v4(0, 0, &mut out);
        assert_eq!(out, "00000000-0000-4000-8000-000000000000");
    }
}
