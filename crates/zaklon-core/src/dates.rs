//! Dates and times in the forms Zaklon stores and shows: RFC 3339 timestamps
//! in UTC, calendar dates as "YYYY-MM-DD" in local time, and compact stamps
//! for file names. One place, so every part of the hub writes them the same way.

use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime};

const DATE: &[time::format_description::BorrowedFormatItem<'static>] = format_description!("[year]-[month]-[day]");

/// RFC 3339 timestamp in UTC, second precision ("2026-09-27T18:50:51Z").
pub fn now_rfc3339() -> String {
    rfc3339(OffsetDateTime::now_utc())
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.replace_nanosecond(0).unwrap_or(t).format(&Rfc3339).unwrap_or_default()
}

/// UTC time for file names, sortable by name: "2026-09-27-185051".
pub fn file_stamp() -> String {
    file_stamp_of(OffsetDateTime::now_utc())
}

fn file_stamp_of(t: OffsetDateTime) -> String {
    t.format(format_description!("[year]-[month]-[day]-[hour][minute][second]")).unwrap_or_default()
}

/// Today's date on this computer (local time), "YYYY-MM-DD".
pub fn today() -> String {
    format_date(OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc()).date())
}

/// A real calendar date written as YYYY-MM-DD (rejects 2027-02-31).
pub fn is_date(d: &str) -> bool {
    parse_date(d).is_some()
}

/// `date` moved by `days`; an unreadable date is returned as it is.
pub fn plus_days(date: &str, days: i64) -> String {
    match parse_date(date) {
        Some(d) => format_date(d.saturating_add(time::Duration::days(days))),
        None => date.to_string(),
    }
}

fn parse_date(d: &str) -> Option<Date> {
    (d.len() == 10).then(|| Date::parse(d, DATE).ok()).flatten()
}

fn format_date(d: Date) -> String {
    d.format(DATE).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_to_the_second() {
        let t = OffsetDateTime::from_unix_timestamp(0).unwrap();
        assert_eq!(rfc3339(t), "1970-01-01T00:00:00Z");
        let leap = time::macros::datetime!(2000-02-29 23:59:59.75 UTC);
        assert_eq!(rfc3339(leap), "2000-02-29T23:59:59Z");
        assert_eq!(file_stamp_of(leap), "2000-02-29-235959");
        assert_eq!(file_stamp_of(time::macros::datetime!(2100-03-01 01:02:03 UTC)), "2100-03-01-010203");
        let now = now_rfc3339();
        assert!(OffsetDateTime::parse(&now, &Rfc3339).is_ok(), "{now}");
        assert!(now.ends_with('Z') && !now.contains('.'), "{now}");
    }

    #[test]
    fn calendar_dates() {
        assert!(is_date("2028-02-29"));
        assert!(!is_date("2027-02-29"));
        assert!(!is_date("2027-02-31"));
        assert!(!is_date("31.12.2026"));
        assert!(!is_date("2026-1-01"));
        assert_eq!(plus_days("2026-12-31", 1), "2027-01-01");
        assert_eq!(plus_days("2028-03-01", -1), "2028-02-29");
        assert_eq!(plus_days("not a date", 3), "not a date");
        assert!(is_date(&today()));
    }
}
