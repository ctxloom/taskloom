//! The proleptic Gregorian calendar in UTC, as the use cases read and
//! spell instants: a date (`YYYY-MM-DD`) at midnight, as a prune's cut
//! takes one; an instant in RFC 3339 with nanoseconds, as Go spells it in
//! an import's repair key. Howard Hinnant's day arithmetic, both ways.

use ports::Timestamp;

const SECONDS_PER_DAY: i64 = 86_400;

/// `text` as a date, `YYYY-MM-DD`, at midnight UTC.
pub(crate) fn date(text: &str) -> Option<Timestamp> {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days_in_month).contains(&day) {
        return None;
    }
    Some(Timestamp {
        seconds: days_from_civil(year, month, day) * SECONDS_PER_DAY,
        nanos: 0,
    })
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `at` in UTC in RFC 3339 with nanoseconds: the fraction's trailing zeros
/// dropped, and the fraction too when it is zero.
pub(crate) fn rfc3339_nano(at: Timestamp) -> String {
    let (year, month, day) = civil(at.seconds.div_euclid(86_400));
    let second = at.seconds.rem_euclid(86_400);
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        second / 3600,
        second % 3600 / 60,
        second % 60
    );
    if at.nanos != 0 {
        let fraction = format!("{:09}", at.nanos);
        text.push('.');
        text.push_str(fraction.trim_end_matches('0'));
    }
    text.push('Z');
    text
}

/// The proleptic Gregorian date `days` after 1970-01-01: Howard Hinnant's
/// `civil_from_days`.
fn civil(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
#[path = "calendar.test.rs"]
mod tests;
