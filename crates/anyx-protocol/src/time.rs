//! `Instant` and `Duration` as written on the wire.
//!
//! An instant is RFC 3339 text in UTC, to the millisecond:
//! `2026-11-14T06:00:08.000Z`. A duration is a run of whole numbers, each
//! followed by its unit, from the largest unit down: `1 hour 30 minutes`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MILLISECONDS_PER_DAY: i64 = 86_400_000;

/// The length in milliseconds of each unit a duration may have: the
/// variants of `Duration` in the prelude, which names them.
fn unit_length(unit: &str) -> Option<u64> {
    Some(match unit {
        "day" => 86_400_000,
        "hour" => 3_600_000,
        "minute" => 60_000,
        "second" => 1_000,
        "millisecond" => 1,
        _ => return None,
    })
}

/// Checks the variants of `Duration` as the prelude declares them: each a
/// unit with a length, largest first.
pub(crate) fn check_units(variants: &[String]) -> Result<(), String> {
    let mut previous: Option<u64> = None;
    for unit in variants {
        let Some(length) = unit_length(unit) else {
            return Err(format!(
                "found the unit `{unit}` of `Duration`, whose length the implementation doesn't know"
            ));
        };
        if previous.is_some_and(|previous| length >= previous) {
            return Err(format!(
                "found the unit `{unit}` of `Duration` after a shorter or as long a unit; expected the units largest first"
            ));
        }
        previous = Some(length);
    }
    Ok(())
}

/// A unit of a duration: written in the singular for 1 and the plural,
/// the singular and `s`, otherwise.
pub(crate) struct Unit {
    pub singular: &'static str,
    pub plural: String,
    pub milliseconds: u64,
}

/// The units of a duration, largest first: the variants of `Duration` in
/// the prelude.
pub(crate) fn units() -> &'static [Unit] {
    static UNITS: std::sync::LazyLock<Vec<Unit>> = std::sync::LazyLock::new(|| {
        crate::prelude::of(&crate::schema::Type::Duration)
            .map_or(&[][..], |duration| &duration.variants)
            .iter()
            .map(|unit| Unit {
                singular: unit.as_str(),
                plural: format!("{unit}s"),
                milliseconds: unit_length(unit).unwrap_or(1),
            })
            .collect()
    });
    &UNITS
}

/// Whether a shape is a duration's, a unit after each slot: `_ hour`,
/// `_ hours _ minutes`.
pub(crate) fn is_duration_shape(parts: &[crate::schema::ShapePart]) -> bool {
    use crate::schema::ShapePart;
    !parts.is_empty()
        && parts.chunks(2).all(|pair| match pair {
            [ShapePart::Slot, ShapePart::Word(word)] => units()
                .iter()
                .any(|unit| unit.singular == *word || unit.plural == *word),
            _ => false,
        })
}

/// Milliseconds since the Unix epoch, rounded down: what an instant keeps.
pub(crate) fn milliseconds_since_epoch(instant: SystemTime) -> Option<i64> {
    match instant.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_millis()).ok(),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_millis()).ok()?;
            let rest = before.as_nanos() % 1_000_000 != 0;
            Some(-whole - i64::from(rest))
        }
    }
}

fn from_milliseconds_since_epoch(milliseconds: i64) -> Option<SystemTime> {
    let offset = Duration::from_millis(milliseconds.unsigned_abs());
    match milliseconds >= 0 {
        true => UNIX_EPOCH.checked_add(offset),
        false => UNIX_EPOCH.checked_sub(offset),
    }
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The year, month and day of a count of days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
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

fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `instant` as `2026-11-14T06:00:08.000Z`, or `None` outside the years
/// 0000 to 9999 that RFC 3339 can write.
pub(crate) fn format_instant(instant: SystemTime) -> Option<String> {
    let milliseconds = milliseconds_since_epoch(instant)?;
    let days = milliseconds.div_euclid(MILLISECONDS_PER_DAY);
    let in_day = milliseconds.rem_euclid(MILLISECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    if !(0..=9999).contains(&year) {
        return None;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1_000 % 60,
        in_day % 1_000
    ))
}

/// Reads RFC 3339 text in UTC: `2026-11-14T06:00:08Z`, with up to three
/// digits of a second's fraction after a `.`.
pub(crate) fn parse_instant(text: &str) -> Option<SystemTime> {
    let bytes = text.as_bytes();
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = bytes.get(range)?;
        digits
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| std::str::from_utf8(digits).ok()?.parse().ok())?
    };
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if bytes.len() < 20
        || separators
            .iter()
            .any(|(index, separator)| bytes[*index] != *separator)
    {
        return None;
    }
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    let fraction = match &bytes[19..] {
        b"Z" => 0,
        [b'.', digits @ .., b'Z'] if (1..=3).contains(&digits.len()) => {
            number(20..20 + digits.len())? * 10_i64.pow(3 - digits.len() as u32)
        }
        _ => return None,
    };
    let fits = (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour < 24
        && minute < 60
        && second < 60;
    if !fits {
        return None;
    }
    let milliseconds = days_from_civil(year, month, day) * MILLISECONDS_PER_DAY
        + ((hour * 60 + minute) * 60 + second) * 1_000
        + fraction;
    from_milliseconds_since_epoch(milliseconds)
}

/// `duration` as words, to the millisecond: `1 hour 30 minutes`, or
/// `0 seconds`. `None` when its milliseconds don't fit in 64 bits.
pub(crate) fn format_duration(duration: Duration) -> Option<String> {
    let mut rest = u64::try_from(duration.as_millis()).ok()?;
    let mut parts = Vec::new();
    for unit in units() {
        let count = rest / unit.milliseconds;
        rest %= unit.milliseconds;
        if count > 0 {
            parts.push(format!(
                "{count} {}",
                if count == 1 {
                    unit.singular
                } else {
                    &unit.plural
                }
            ));
        }
    }
    Some(match parts.is_empty() {
        true => "0 seconds".to_string(),
        false => parts.join(" "),
    })
}

/// Reads pairs of a whole number and a unit, such as `[("1", "hour"),
/// ("30", "minutes")]`, each unit once and the largest first. `Err` says
/// what is wrong.
pub(crate) fn parse_duration(pairs: &[(&str, &str)]) -> Result<Duration, String> {
    if pairs.is_empty() {
        return Err("no number and unit".to_string());
    }
    let mut milliseconds: u64 = 0;
    let mut smallest = None;
    for (count, unit) in pairs {
        let Some(position) = units()
            .iter()
            .position(|known| *unit == known.singular || *unit == known.plural)
        else {
            return Err(format!("the unit `{unit}`"));
        };
        if smallest.is_some_and(|smallest| position <= smallest) {
            return Err(format!("`{unit}` after a smaller or the same unit"));
        }
        smallest = Some(position);
        let count: u64 = count
            .parse()
            .map_err(|_| format!("`{count}`, which is not a whole number from 0"))?;
        milliseconds = count
            .checked_mul(units()[position].milliseconds)
            .and_then(|part| milliseconds.checked_add(part))
            .ok_or_else(|| "a duration too long to hold".to_string())?;
    }
    Ok(Duration::from_millis(milliseconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants_read_back_as_written() {
        for text in [
            "1970-01-01T00:00:00.000Z",
            "2026-11-14T06:00:08.123Z",
            "2000-02-29T23:59:59.999Z",
            "1969-12-31T23:59:59.999Z",
            "0000-01-01T00:00:00.000Z",
            "9999-12-31T23:59:59.999Z",
        ] {
            let instant = parse_instant(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(format_instant(instant).as_deref(), Some(text));
        }
    }

    #[test]
    fn an_instant_without_a_fraction_reads() {
        let instant = parse_instant("2026-11-14T06:00:08Z").unwrap();
        assert_eq!(format_instant(instant).unwrap(), "2026-11-14T06:00:08.000Z");
        assert_eq!(
            format_instant(parse_instant("2026-11-14T06:00:08.5Z").unwrap()).unwrap(),
            "2026-11-14T06:00:08.500Z"
        );
    }

    #[test]
    fn text_that_is_no_instant_in_utc_does_not_read() {
        for text in [
            "2026-11-14",
            "2026-11-14T06:00:08",
            "2026-11-14T06:00:08+08:00",
            "2026-11-14t06:00:08z",
            "2026-02-30T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-11-14T24:00:00Z",
            "2026-11-14T06:00:60Z",
            "2026-11-14T06:00:08.1234Z",
            "2026-11-14T06:00:08.Z",
            "+2026-11-14T06:00:08Z",
        ] {
            assert_eq!(parse_instant(text), None, "{text}");
        }
    }

    #[test]
    fn an_instant_rounds_down_to_the_millisecond() {
        let after = UNIX_EPOCH + Duration::from_nanos(1_999_999);
        assert_eq!(format_instant(after).unwrap(), "1970-01-01T00:00:00.001Z");
        let before = UNIX_EPOCH - Duration::from_nanos(1);
        assert_eq!(format_instant(before).unwrap(), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn durations_are_written_from_the_largest_unit() {
        let cases = [
            (0, "0 seconds"),
            (1, "1 millisecond"),
            (90_000, "1 minute 30 seconds"),
            (5_400_000, "1 hour 30 minutes"),
            (2 * 86_400_000 + 1_001, "2 days 1 second 1 millisecond"),
        ];
        for (milliseconds, text) in cases {
            assert_eq!(
                format_duration(Duration::from_millis(milliseconds)).as_deref(),
                Some(text)
            );
        }
    }

    #[test]
    fn durations_read_in_any_count_of_their_units() {
        assert_eq!(
            parse_duration(&[("90", "seconds")]),
            Ok(Duration::from_secs(90))
        );
        assert_eq!(
            parse_duration(&[("1", "hours"), ("1", "minute")]),
            Ok(Duration::from_secs(3660))
        );
        assert!(parse_duration(&[("1", "minute"), ("1", "hour")]).is_err());
        assert!(parse_duration(&[("1", "minute"), ("1", "minute")]).is_err());
        assert!(parse_duration(&[("1.5", "hours")]).is_err());
        assert!(parse_duration(&[("-1", "hours")]).is_err());
        assert!(parse_duration(&[("1", "weeks")]).is_err());
    }
}
