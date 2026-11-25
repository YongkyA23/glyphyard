//! Usage statistics and badges (Task 3).

use super::*;
use chrono::{DateTime, Duration, Utc};

fn at(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Writes a usage row directly, so a test controls when it happened.
fn log(fx: &Fixture, key: &str, active: bool, t: DateTime<Utc>) {
    fx.engine
        .db()
        .execute(
            "INSERT INTO usage_log (family_key, hash, active, source, at) VALUES (?1, 'h', ?2, 'user', ?3)",
            rusqlite::params![key, active, at(t)],
        )
        .unwrap();
}

fn added(fx: &Fixture, t: DateTime<Utc>) {
    fx.engine.db().execute("UPDATE files SET added_at = ?1", [at(t)]).unwrap();
}

/// Pretends recording started at `t`.
fn recording_since(fx: &Fixture, t: DateTime<Utc>) {
    fx.engine.db().execute("UPDATE settings SET value = ?1 WHERE key = 'usage_since'", [format!("\"{}\"", at(t))]).unwrap();
}

fn usage_rows(fx: &Fixture) -> Vec<(bool, String)> {
    let db = fx.engine.db();
    let mut stmt = db.prepare("SELECT active, source FROM usage_log ORDER BY id").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
}

fn earned(fx: &Fixture, id: &str) -> Option<String> {
    fx.engine.badges(Utc::now()).unwrap().into_iter().find(|b| b.id == id).unwrap().earned_at
}

fn ui_events(fx: &Fixture) -> Arc<Mutex<Vec<UiEvent>>> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    fx.engine.on_ui_event(move |e| sink.lock().unwrap().push(e));
    events
}

fn badge_events(events: &Mutex<Vec<UiEvent>>) -> Vec<String> {
    events.lock().unwrap().iter().filter_map(|e| if let UiEvent::BadgeEarned(b) = e { Some(b.id.to_string()) } else { None }).collect()
}

/// Arial and Times New Roman in the library, or None when the OS lacks Times.
fn two_families(fx: &Fixture) -> Option<(String, String)> {
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.put("times.ttf", &system_font("times.ttf")?);
    fx.engine.add_folder(&fx.root).unwrap();
    Some((fx.family("Arial").key, fx.family("Times New Roman").key))
}

#[test]
fn most_used_ranks_by_distinct_days() {
    let fx = fixture!();
    let Some((arial, times)) = two_families(&fx) else { return };
    let now: DateTime<Utc> = "2026-11-20T12:00:00Z".parse().unwrap();
    // Arial: four times on one day. Times: once on each of two days.
    for h in 0..4 {
        log(&fx, &arial, true, now - Duration::hours(h));
    }
    log(&fx, &times, true, now - Duration::days(1));
    log(&fx, &times, true, now - Duration::days(2));
    // Turning off is not use; last month only counts for the year.
    log(&fx, &arial, false, now - Duration::days(3));
    log(&fx, &arial, true, "2026-10-05T10:00:00Z".parse().unwrap());
    log(&fx, &arial, true, "2026-10-06T10:00:00Z".parse().unwrap());

    let month = fx.engine.usage_stats(UsagePeriod::Month, now).unwrap();
    let ranked: Vec<(&str, u32)> = month.most_used.iter().map(|f| (f.key.as_str(), f.days)).collect();
    assert_eq!(ranked, vec![(times.as_str(), 2), (arial.as_str(), 1)]);
    assert_eq!(month.most_used[0].family, "Times New Roman");

    let year = fx.engine.usage_stats(UsagePeriod::Year, now).unwrap();
    let ranked: Vec<(&str, u32)> = year.most_used.iter().map(|f| (f.key.as_str(), f.days)).collect();
    assert_eq!(ranked, vec![(arial.as_str(), 3), (times.as_str(), 2)]);
    assert_eq!(year.total_fonts, 2);
}

#[test]
fn never_used_needs_a_library_older_than_30_days() {
    let fx = fixture!();
    let Some((arial, times)) = two_families(&fx) else { return };
    let now = Utc::now();

    let young = fx.engine.usage_stats(UsagePeriod::Year, now).unwrap();
    assert!(young.never_used.is_empty());
    assert!(young.never_used_note.is_some(), "says why the list is empty");
    assert_eq!(young.added_in_period, 2);

    added(&fx, now - Duration::days(400));
    recording_since(&fx, now - Duration::days(400));
    log(&fx, &arial, true, now - Duration::days(10));
    log(&fx, &times, true, now - Duration::days(380)); // more than 12 months ago: still never used
    let old = fx.engine.usage_stats(UsagePeriod::Year, now).unwrap();
    assert_eq!(old.never_used.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), vec![times.as_str()]);
    assert!(old.never_used_note.is_none());
    assert_eq!(old.added_in_period, 0);

    // A family that is on right now is in use, whatever the log says.
    fx.engine.set_active(&fx.hashes("Times New Roman"), true).unwrap();
    fx.engine.db().execute("DELETE FROM usage_log", []).unwrap();
    let on = fx.engine.usage_stats(UsagePeriod::Year, now).unwrap();
    assert_eq!(on.never_used.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), vec![arial.as_str()]);
}

#[test]
fn never_used_skips_families_added_in_the_last_30_days() {
    let fx = fixture!();
    let Some((arial, _times)) = two_families(&fx) else { return };
    let now = Utc::now();
    added(&fx, now - Duration::days(400));
    recording_since(&fx, now - Duration::days(400));
    let times = fx.hashes("Times New Roman");
    fx.engine.db().execute("UPDATE files SET added_at = ?1 WHERE hash = ?2", rusqlite::params![at(now - Duration::days(5)), times[0]]).unwrap();
    let stats = fx.engine.usage_stats(UsagePeriod::Year, now).unwrap();
    assert_eq!(stats.never_used.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), vec![arial.as_str()], "Times arrived last week");
}

#[test]
fn never_used_waits_for_30_days_of_recording() {
    let fx = fixture!();
    let Some(_) = two_families(&fx) else { return };
    let now = Utc::now();
    added(&fx, now - Duration::days(400));
    // Recording started today (a library upgraded to this version): nothing is known yet.
    let stats = fx.engine.usage_stats(UsagePeriod::Year, now).unwrap();
    assert!(stats.never_used.is_empty());
    assert!(stats.never_used_note.is_some());
    recording_since(&fx, now - Duration::days(31));
    assert_eq!(fx.engine.usage_stats(UsagePeriod::Year, now).unwrap().never_used.len(), 2);
}

#[test]
fn renaming_a_family_keeps_its_usage() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let now = Utc::now();
    log(&fx, &fx.family("Arial").key, true, now);
    fx.engine.set_family_name(&fx.hashes("Arial"), Some("Renamed")).unwrap();
    let stats = fx.engine.usage_stats(UsagePeriod::Year, now).unwrap();
    assert_eq!(stats.most_used.iter().map(|f| f.family.as_str()).collect::<Vec<_>>(), vec!["Renamed"]);
}

#[test]
fn user_changes_are_recorded_and_admin_ones_are_not() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let font = arial_ref(&fx);

    fx.engine.apply_command(&CommandKind::Activate { font: font.clone() }).unwrap();
    fx.engine.apply_command(&CommandKind::Deactivate { font: font.clone() }).unwrap();
    assert!(usage_rows(&fx).is_empty(), "admin-pushed changes are not the user's use");

    fx.engine.set_active(&fx.hashes("Arial"), true).unwrap();
    fx.engine.set_active(&fx.hashes("Arial"), false).unwrap();
    assert_eq!(usage_rows(&fx), vec![(true, "user".to_string()), (false, "user".to_string())]);
}

#[test]
fn collector_is_earned_once_at_1000_fonts() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let events = ui_events(&fx);
    assert_eq!(earned(&fx, "collector"), None);
    {
        let db = fx.engine.db();
        for n in 0..999 {
            db.execute(
                "INSERT INTO files (folder_id, rel_path, hash, size, mtime, format, added_at)
                 SELECT folder_id, ?1, ?1, 1, 1, 'ttf', added_at FROM files LIMIT 1",
                [format!("fake{n}")],
            )
            .unwrap();
        }
    }
    let first = earned(&fx, "collector").expect("1,000 fonts");
    assert_eq!(earned(&fx, "collector"), Some(first), "awarded once, the date stays");
    assert_eq!(badge_events(&events), vec!["collector"]);
}

#[test]
fn archaeologist_needs_a_font_unused_for_a_year() {
    let fx = fixture!();
    let Some((arial, times)) = two_families(&fx) else { return };
    let now = Utc::now();
    added(&fx, now - Duration::days(800));
    recording_since(&fx, now - Duration::days(700));
    log(&fx, &arial, true, now - Duration::days(400));
    log(&fx, &times, true, now - Duration::days(100));
    // That history was checked when it happened.
    fx.engine.db().execute("INSERT OR REPLACE INTO settings (key, value) SELECT 'usage_seen', MAX(id) FROM usage_log", []).unwrap();
    let events = ui_events(&fx);

    fx.engine.set_active(&fx.hashes("Times New Roman"), true).unwrap();
    assert_eq!(earned(&fx, "archaeologist"), None, "Times was used 100 days ago");
    fx.engine.set_active(&fx.hashes("Arial"), true).unwrap();
    assert!(earned(&fx, "archaeologist").is_some());
    assert_eq!(badge_events(&events), vec!["archaeologist"]);
}

#[test]
fn archaeologist_waits_until_a_year_has_been_recorded() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    added(&fx, Utc::now() - Duration::days(800));
    // Recording started today (a library upgraded to this version): no way to know it went unused.
    fx.engine.set_active(&fx.hashes("Arial"), true).unwrap();
    assert_eq!(earned(&fx, "archaeologist"), None);
}

const ELEVEN: [&str; 11] = [
    "arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf", "times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf", "cour.ttf", "courbd.ttf", "couri.ttf",
];

/// Eleven fonts with different PostScript names, all on; None when the OS lacks some.
fn eleven_on(fx: &Fixture) -> Option<Vec<String>> {
    for name in ELEVEN {
        fx.put(name, &system_font(name)?);
    }
    fx.engine.add_folder(&fx.root).unwrap();
    let hashes: Vec<String> = fx.engine.list_families(&FamilyQuery::default()).unwrap().into_iter().flat_map(|f| f.faces).map(|f| f.hash).collect();
    assert_eq!(hashes.len(), 11);
    assert!(fx.engine.set_active(&hashes, true).unwrap().iter().all(|r| r.ok));
    Some(hashes)
}

#[test]
fn quiet_is_every_font_off_at_once() {
    let fx = fixture!();
    let Some(hashes) = eleven_on(&fx) else { return };
    let events = ui_events(&fx);
    fx.engine.set_active(&hashes, false).unwrap();
    assert!(earned(&fx, "quiet").is_some());
    assert_eq!(badge_events(&events), vec!["quiet"]);
}

#[test]
fn quiet_is_not_one_by_one_or_some() {
    let fx = fixture!();
    let Some(hashes) = eleven_on(&fx) else { return };
    fx.engine.set_active(&hashes[..10], false).unwrap();
    assert_eq!(earned(&fx, "quiet"), None, "ten at once, but one font is still on");
    fx.engine.set_active(&hashes[10..], false).unwrap();
    assert_eq!(earned(&fx, "quiet"), None, "the last one alone is not ten at once");
}
