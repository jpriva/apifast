//! Test data for dynamic variables (`{{$randomEmail}}`…), with the names
//! Postman and Bruno use, so imported collections work unchanged.
//!
//! Small fixed lists are enough for test data; randomness comes from the
//! [`Host`], so a test host makes them repeatable.

use std::fmt::Write as _;

use crate::Host;

const FIRST_NAMES: &[&str] = &[
    "Ana", "Bruno", "Carla", "Diego", "Elena", "Felipe", "Grace", "Hugo", "Irene", "Javier",
    "Karen", "Luis", "Marta", "Nora", "Omar", "Paula", "Quentin", "Rosa", "Samuel", "Tania",
    "Ulises", "Valeria", "Walter", "Ximena", "Yusuf", "Zoe",
];

const LAST_NAMES: &[&str] = &[
    "Alvarez",
    "Brown",
    "Castro",
    "Diaz",
    "Evans",
    "Fernandez",
    "Garcia",
    "Hughes",
    "Ito",
    "Jensen",
    "Kim",
    "Lopez",
    "Martin",
    "Nakamura",
    "Ortiz",
    "Perez",
    "Quinn",
    "Rossi",
    "Silva",
    "Torres",
    "Ueda",
    "Vargas",
    "Weber",
    "Xu",
    "Young",
    "Zimmer",
];

const WORDS: &[&str] = &[
    "amber", "anchor", "basil", "beacon", "cedar", "comet", "delta", "ember", "falcon", "fern",
    "garnet", "harbor", "indigo", "jasper", "kettle", "lantern", "maple", "meadow", "nectar",
    "orbit", "pebble", "quartz", "river", "saffron", "summit", "thistle", "umber", "velvet",
    "willow", "yonder", "zephyr",
];

const CITIES: &[&str] = &[
    "Lima",
    "Bogota",
    "Madrid",
    "Lisbon",
    "Berlin",
    "Osaka",
    "Toronto",
    "Nairobi",
    "Sydney",
    "Santiago",
    "Oslo",
    "Quito",
    "Seoul",
    "Dublin",
    "Mexico City",
    "Montevideo",
];

const COUNTRIES: &[&str] = &[
    "Peru",
    "Colombia",
    "Spain",
    "Portugal",
    "Germany",
    "Japan",
    "Canada",
    "Kenya",
    "Australia",
    "Chile",
    "Norway",
    "Ecuador",
    "South Korea",
    "Ireland",
    "Mexico",
    "Uruguay",
];

const ALPHANUMERIC: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

fn pick<'a>(host: &mut dyn Host, list: &[&'a str]) -> &'a str {
    let len = list.len() as u64;
    // `len` is small, so the index fits and the bias is negligible.
    #[expect(clippy::cast_possible_truncation, reason = "below the list's length")]
    let index = (host.random_u64() % len) as usize;
    list[index]
}

fn number(host: &mut dyn Host, below: u64) -> u64 {
    host.random_u64() % below
}

/// Writes the value of test-data variable `name` into `out`. Returns
/// `false` if it is not one.
pub(crate) fn generate(name: &str, host: &mut dyn Host, out: &mut String) -> bool {
    match name {
        "randomFirstName" => out.push_str(pick(host, FIRST_NAMES)),
        "randomLastName" => out.push_str(pick(host, LAST_NAMES)),
        "randomFullName" => {
            let first = pick(host, FIRST_NAMES);
            let last = pick(host, LAST_NAMES);
            let _ = write!(out, "{first} {last}");
        }
        "randomUserName" => {
            let first = pick(host, FIRST_NAMES).to_ascii_lowercase();
            let last = pick(host, LAST_NAMES).to_ascii_lowercase();
            let _ = write!(out, "{first}.{last}{}", number(host, 100));
        }
        // `example.com` is reserved for examples: nothing is ever sent there.
        "randomEmail" => {
            let first = pick(host, FIRST_NAMES).to_ascii_lowercase();
            let last = pick(host, LAST_NAMES).to_ascii_lowercase();
            let _ = write!(out, "{first}.{last}{}@example.com", number(host, 1000));
        }
        "randomWord" => out.push_str(pick(host, WORDS)),
        "randomWords" => {
            let words: Vec<&str> = (0..3).map(|_| pick(host, WORDS)).collect();
            out.push_str(&words.join(" "));
        }
        "randomCity" => out.push_str(pick(host, CITIES)),
        "randomCountry" => out.push_str(pick(host, COUNTRIES)),
        "randomBoolean" => out.push_str(if host.random_u64().is_multiple_of(2) {
            "false"
        } else {
            "true"
        }),
        "randomAlphaNumeric" => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "below the alphabet's length"
            )]
            let index = number(host, ALPHANUMERIC.len() as u64) as usize;
            out.push(char::from(ALPHANUMERIC[index]));
        }
        "randomPhoneNumber" => {
            let _ = write!(
                out,
                "{:03}-{:03}-{:04}",
                200 + number(host, 800),
                number(host, 1000),
                number(host, 10_000)
            );
        }
        _ => return false,
    }
    true
}
