//! Dates in cookies (RFC 6265, 5.1.1): the forgiving algorithm browsers
//! use, which reads `Wed, 21 Oct 2015 07:28:00 GMT`, `Wednesday,
//! 21-Oct-15 07:28:00 GMT` and `Wed Oct 21 07:28:00 2015` alike.

/// Milliseconds since the Unix epoch of the cookie date `text`, or `None`
/// if it is not one.
pub(crate) fn parse_cookie_date(text: &str) -> Option<i64> {
    let mut time = None;
    let mut day = None;
    let mut month = None;
    let mut year = None;
    for token in text.split(is_delimiter).filter(|t| !t.is_empty()) {
        if time.is_none()
            && let Some(t) = parse_time(token)
        {
            time = Some(t);
        } else if day.is_none()
            && let Some(d) = leading_number(token, 1, 2)
        {
            day = Some(d);
        } else if month.is_none()
            && let Some(m) = parse_month(token)
        {
            month = Some(m);
        } else if year.is_none()
            && let Some(y) = leading_number(token, 2, 4)
        {
            year = Some(y);
        }
    }
    let (hour, minute, second) = time?;
    let (day, month) = (day?, month?);
    let year = match year? {
        y @ 70..=99 => y + 1900,
        y @ 0..=69 => y + 2000,
        y => y,
    };
    let valid = year >= 1601
        && (1..=days_in_month(year, month)).contains(&day)
        && hour <= 23
        && minute <= 59
        && second <= 59;
    if !valid {
        return None;
    }
    let days = days_from_civil(i64::from(year), month, day);
    let seconds = days * 86_400 + i64::from(hour * 3600 + minute * 60 + second);
    Some(seconds * 1000)
}

/// `ms` since the Unix epoch as `YYYY-MM-DD HH:MM:SS`, in UTC: how the
/// cookie editor shows an expiry.
pub fn format_expiry(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let (days, of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// The `YYYY-MM-DD HH:MM:SS` (or `YYYY-MM-DD`, at midnight) of
/// [`format_expiry`] back to milliseconds, in UTC.
pub fn parse_expiry(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, time) = text.split_once([' ', 'T']).unwrap_or((text, "00:00:00"));
    let mut date_parts = date.split('-');
    let year: u32 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.trim().split(':');
    let hour: u32 = time_parts.next()?.parse().ok()?;
    let minute: u32 = time_parts.next().unwrap_or("0").parse().ok()?;
    let second: u32 = time_parts.next().unwrap_or("0").parse().ok()?;
    let valid = date_parts.next().is_none()
        && time_parts.next().is_none()
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour <= 23
        && minute <= 59
        && second <= 59;
    valid.then(|| {
        let days = days_from_civil(i64::from(year), month, day);
        (days * 86_400 + i64::from(hour * 3600 + minute * 60 + second)) * 1000
    })
}

/// The date of `days` since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The delimiters of RFC 6265: tab, space to `/`, `;` to `@`, `[` to `` ` ``
/// and `{` to `~`.
fn is_delimiter(c: char) -> bool {
    matches!(c, '\t' | ' '..='/' | ';'..='@' | '['..='`' | '{'..='~')
}

/// `hh:mm:ss`, each with one or two digits, maybe followed by a non-digit
/// and anything.
fn parse_time(token: &str) -> Option<(u32, u32, u32)> {
    let mut parts = token.splitn(3, ':');
    let hour = whole_number(parts.next()?)?;
    let minute = whole_number(parts.next()?)?;
    let second = leading_number(parts.next()?, 1, 2)?;
    Some((hour, minute, second))
}

/// One or two digits and nothing else.
fn whole_number(text: &str) -> Option<u32> {
    (matches!(text.len(), 1 | 2) && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// The `min` to `max` digits at the start of `text`, if a non-digit (or the
/// end) follows them.
fn leading_number(text: &str, min: usize, max: usize) -> Option<u32> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    (min..=max)
        .contains(&digits)
        .then(|| text[..digits].parse().ok())
        .flatten()
}

/// The month whose three-letter name starts `token`, from 1.
fn parse_month(token: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let start = token.get(..3)?.to_ascii_lowercase();
    let index = MONTHS.iter().position(|m| *m == start)?;
    u32::try_from(index + 1).ok()
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar
/// (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2015-10-21T07:28:00Z.
    const BACK_TO_THE_FUTURE: i64 = 1_445_412_480_000;

    #[test]
    fn the_usual_formats_are_read() {
        for text in [
            "Wed, 21 Oct 2015 07:28:00 GMT",
            "Wednesday, 21-Oct-15 07:28:00 GMT",
            "Wed Oct 21 07:28:00 2015",
            "21 oct 2015 7:28:0",
            "Wed, 21-OCT-2015 07:28:00 UTC",
        ] {
            assert_eq!(parse_cookie_date(text), Some(BACK_TO_THE_FUTURE), "{text}");
        }
    }

    #[test]
    fn two_digit_years_are_1970_to_2069() {
        assert_eq!(parse_cookie_date("01 Jan 70 00:00:00"), Some(0));
        assert_eq!(
            parse_cookie_date("01 Jan 69 00:00:00"),
            parse_cookie_date("01 Jan 2069 00:00:00")
        );
    }

    #[test]
    fn dates_that_do_not_exist_are_not_read() {
        for text in [
            "",
            "Wed, 21 Oct 2015",
            "30 Feb 2015 00:00:00",
            "29 Feb 2015 00:00:00",
            "21 Oct 1600 00:00:00",
            "21 Oct 2015 24:00:00",
            "21 Oct 2015 07:60:00",
            "32 Oct 2015 07:28:00",
            "21 Foo 2015 07:28:00",
        ] {
            assert_eq!(parse_cookie_date(text), None, "{text}");
        }
        assert!(parse_cookie_date("29 Feb 2016 00:00:00").is_some());
        assert!(parse_cookie_date("29 Feb 2000 00:00:00").is_some());
    }

    #[test]
    fn expiries_are_written_and_read_back() {
        assert_eq!(format_expiry(BACK_TO_THE_FUTURE), "2015-10-21 07:28:00");
        assert_eq!(format_expiry(0), "1970-01-01 00:00:00");
        assert_eq!(format_expiry(-1000), "1969-12-31 23:59:59");
        assert_eq!(format_expiry(951_782_400_000), "2000-02-29 00:00:00");
        for ms in [0, BACK_TO_THE_FUTURE, 951_782_400_000, 4_102_444_799_000] {
            assert_eq!(parse_expiry(&format_expiry(ms)), Some(ms));
        }
        assert_eq!(parse_expiry("2015-10-21T07:28"), Some(BACK_TO_THE_FUTURE));
        assert_eq!(parse_expiry(" 1970-01-02 "), Some(86_400_000));
        for bad in [
            "",
            "soon",
            "2015-13-01",
            "2015-02-29",
            "2015-10-21 24:00",
            "2015-10-21-1",
        ] {
            assert_eq!(parse_expiry(bad), None, "{bad}");
        }
    }

    #[test]
    fn days_match_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(1601, 1, 1), -134_774);
    }
}
