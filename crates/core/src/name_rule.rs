//! "A name can be changed once every N days." Enforced by the server; the desktop uses it for display.

use chrono::{DateTime, Duration, Utc};

/// When the next change is allowed. `None` means a change is allowed now.
///
/// `last_change` is the last real change (the first name entered at onboarding does not count).
/// An admin reset newer than the last change lifts the limit.
pub fn next_allowed(
    now: DateTime<Utc>,
    last_change: Option<DateTime<Utc>>,
    last_reset: Option<DateTime<Utc>>,
    days: u32,
) -> Option<DateTime<Utc>> {
    let last = last_change?;
    if last_reset.is_some_and(|r| r > last) {
        return None;
    }
    let next = last + Duration::days(days as i64);
    (now < next).then_some(next)
}

/// A name must have visible characters and fit in a table cell.
pub fn validate_name(name: &str) -> Result<String, &'static str> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("name is empty");
    }
    if name.chars().count() > 80 {
        return Err("name is longer than 80 characters");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn day(d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, d, 10, 0, 0).unwrap()
    }

    #[test]
    fn first_change_is_free() {
        assert_eq!(next_allowed(day(1), None, None, 30), None);
    }

    #[test]
    fn locked_for_n_days() {
        let next = next_allowed(day(20), Some(day(12)), None, 30).unwrap();
        assert_eq!(next, day(12) + Duration::days(30));
        assert_eq!(next_allowed(next, Some(day(12)), None, 30), None);
    }

    #[test]
    fn admin_reset_lifts_limit_only_if_newer() {
        assert_eq!(next_allowed(day(20), Some(day(12)), Some(day(15)), 30), None);
        assert!(next_allowed(day(20), Some(day(12)), Some(day(10)), 30).is_some());
    }

    #[test]
    fn names_are_tidied() {
        assert_eq!(validate_name("  Budi   Santoso "), Ok("Budi Santoso".into()));
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(81)).is_err());
    }
}
