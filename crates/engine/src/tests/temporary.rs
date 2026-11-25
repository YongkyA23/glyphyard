//! Temporary activation: fonts that turn themselves off after a while or at the next start.

use super::*;
use chrono::{Duration, Utc};

fn arial(fx: &Fixture) -> Vec<String> {
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    fx.hashes("Arial")
}

#[test]
fn temporary_font_turns_off_after_its_time_and_not_before() {
    let fx = fixture!();
    let hashes = arial(&fx);
    assert!(fx.engine.set_active_for(&hashes, Until::Minutes(120)).unwrap().iter().all(|r| r.ok));
    assert_eq!(fx.os.registered().len(), 1);
    let face = &fx.family("Arial").faces[0];
    assert!(face.temporary_until.as_deref().is_some_and(|t| t != "restart"), "{:?}", face.temporary_until);

    assert_eq!(fx.engine.expire_temporary(Utc::now() + Duration::minutes(100), false).unwrap(), 0);
    assert_eq!(fx.os.registered().len(), 1);
    assert_eq!(fx.engine.expire_temporary(Utc::now() + Duration::minutes(121), false).unwrap(), 1);
    assert!(fx.os.registered().is_empty());
    assert_eq!(fx.family("Arial").faces[0].temporary_until, None);
}

#[test]
fn until_restart_survives_timed_sweeps_and_goes_at_start_up() {
    let fx = fixture!();
    let hashes = arial(&fx);
    fx.engine.set_active_for(&hashes, Until::Restart).unwrap();
    assert_eq!(fx.family("Arial").faces[0].temporary_until.as_deref(), Some("restart"));

    assert_eq!(fx.engine.expire_temporary(Utc::now() + Duration::days(30), false).unwrap(), 0);
    assert_eq!(fx.os.registered().len(), 1);
    assert_eq!(fx.engine.expire_temporary(Utc::now(), true).unwrap(), 1);
    assert!(fx.os.registered().is_empty());
}

#[test]
fn turning_on_or_off_normally_clears_the_record() {
    let fx = fixture!();
    let hashes = arial(&fx);
    fx.engine.set_active_for(&hashes, Until::Minutes(30)).unwrap();
    fx.engine.set_active(&hashes, true).unwrap();
    assert_eq!(fx.family("Arial").faces[0].temporary_until, None);
    assert_eq!(fx.engine.expire_temporary(Utc::now() + Duration::hours(3), true).unwrap(), 0);
    assert_eq!(fx.os.registered().len(), 1);

    // Off, then on again later: the old expiry must not come back.
    fx.engine.set_active_for(&hashes, Until::Minutes(30)).unwrap();
    fx.engine.set_active(&hashes, false).unwrap();
    assert_eq!(fx.family("Arial").faces[0].temporary_until, None);
    fx.engine.set_active(&hashes, true).unwrap();
    assert_eq!(fx.engine.expire_temporary(Utc::now() + Duration::hours(3), true).unwrap(), 0);
    assert_eq!(fx.os.registered().len(), 1);
}

#[test]
fn expiry_keeps_fonts_that_another_reason_needs() {
    let fx = fixture!();
    let hashes = arial(&fx);
    // An active project set with the family: the set leaves the already-on font alone, expiry must too.
    fx.engine.set_active_for(&hashes, Until::Minutes(60)).unwrap();
    let key = fx.family("Arial").key;
    fx.engine.save_project_set(None, "Poster", "", std::slice::from_ref(&key)).unwrap();
    let id = fx.engine.list_project_sets().unwrap()[0].id;
    fx.engine.toggle_project_set(id, true).unwrap();
    assert_eq!(fx.engine.expire_temporary(Utc::now() + Duration::hours(2), false).unwrap(), 0);
    assert_eq!(fx.os.registered().len(), 1);
    fx.engine.toggle_project_set(id, false).unwrap();

    // An admin turning on a font that is on temporarily keeps it on.
    fx.engine.set_active_for(&hashes, Until::Restart).unwrap();
    fx.engine.apply_command(&CommandKind::Activate { font: arial_ref(&fx) }).unwrap();
    assert_eq!(fx.engine.expire_temporary(Utc::now(), true).unwrap(), 0);
    assert_eq!(fx.os.registered().len(), 1);
}
