//! RFC 3339 timestamps to Unix seconds, for judging certificate validity at
//! an instant a credential states. Accepts `T` or a space between date and
//! time, optional fractional seconds, and `Z`, a numeric offset or no zone
//! (read as UTC).

/// Seconds since the Unix epoch, or `None` if `s` is not a timestamp.
pub(crate) fn rfc3339_to_unix(s: &str) -> Option<i64> {
    let b = s.trim().as_bytes();
    let num = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = b.get(range)?;
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(digits).ok()?.parse().ok()
    };
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    if second > 60 {
        return None;
    }
    let mut rest = &b[19..];
    if rest.first() == Some(&b'.') {
        let digits = rest[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        rest = &rest[1 + digits..];
    }
    let offset = match rest {
        [] | [b'Z'] | [b'z'] => 0,
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] => {
            let hh = std::str::from_utf8(&[*h1, *h2]).ok()?.parse::<i64>().ok()?;
            let mm = std::str::from_utf8(&[*m1, *m2]).ok()?.parse::<i64>().ok()?;
            let total = hh * 3600 + mm * 60;
            if *sign == b'+' {
                total
            } else {
                -total
            }
        }
        _ => return None,
    };
    // Days from civil (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

#[cfg(test)]
mod tests {
    use super::rfc3339_to_unix;

    #[test]
    fn known_instants_convert() {
        assert_eq!(rfc3339_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_to_unix("2025-10-09T08:53:20Z"), Some(1_760_000_000));
        assert_eq!(
            rfc3339_to_unix("2025-10-09T10:53:20.123+02:00"),
            Some(1_760_000_000)
        );
        assert_eq!(rfc3339_to_unix("2025-10-09 08:53:20"), Some(1_760_000_000));
        assert_eq!(rfc3339_to_unix("2000-02-29T00:00:00Z"), Some(951_782_400));
    }

    #[test]
    fn non_timestamps_are_refused() {
        for s in [
            "",
            "now",
            "2025-13-01T00:00:00Z",
            "2025-10-09T08:53",
            "2025-10-09T08:53:20Q",
        ] {
            assert_eq!(rfc3339_to_unix(s), None, "{s}");
        }
    }
}
