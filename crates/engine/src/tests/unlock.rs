//! Connection unlock password, its throttle, and the language setting.

use super::*;
use argon2::password_hash::PasswordHasher;
use chrono::{Duration, Utc};
use typeshelf_protocol::Policy;

fn locked_policy(e: &Engine, password: &str) -> Policy {
    let mut p = e.policy().unwrap();
    p.allow_connection_edit = false;
    p.connection_password_hash = argon2::Argon2::default().hash_password(password.as_bytes()).unwrap().to_string();
    p
}

fn code(r: Result<()>) -> String {
    r.unwrap_err().code
}

#[test]
fn unlock_requires_a_hash() {
    let fx = fixture!();
    assert_eq!(code(fx.engine.verify_connection_password("x")), "unlock_unavailable");
    assert!(!fx.engine.get_connection().unwrap().unlockable);
}

#[test]
fn unlock_checks_the_password() {
    let fx = fixture!();
    fx.engine.set_policy(&locked_policy(&fx.engine, "correct horse")).unwrap();
    assert_eq!(code(fx.engine.verify_connection_password("wrong")), "wrong_password");
    fx.engine.verify_connection_password("correct horse").unwrap();
    assert!(fx.engine.get_connection().unwrap().unlockable);
}

#[test]
fn unlock_throttle_persists() {
    let fx = fixture!();
    fx.engine.set_policy(&locked_policy(&fx.engine, "correct horse")).unwrap();
    for _ in 0..5 {
        assert_eq!(code(fx.engine.verify_connection_password("nope")), "wrong_password");
    }
    let err = fx.engine.verify_connection_password("correct horse").unwrap_err();
    assert_eq!(err.code, "unlock_throttled");
    assert!(err.extra["retryAfterSecs"].as_i64().unwrap() > 0);

    let Fixture { _tmp, data, os, engine, .. } = fx;
    drop(engine);
    let engine = Engine::open(&data, os).unwrap();
    assert_eq!(code(engine.verify_connection_password("correct horse")), "unlock_throttled");

    let past = serde_json::json!({ "failures": 0, "locked_until": Utc::now() - Duration::minutes(1) });
    store::set(&engine.db(), "unlock_throttle", &past).unwrap();
    engine.verify_connection_password("correct horse").unwrap();
}

#[test]
fn parallel_wrong_passwords_still_lock_out() {
    let fx = fixture!();
    fx.engine.set_policy(&locked_policy(&fx.engine, "correct horse")).unwrap();
    std::thread::scope(|s| {
        for _ in 0..10 {
            s.spawn(|| {
                let _ = fx.engine.verify_connection_password("nope");
            });
        }
    });
    assert_eq!(code(fx.engine.verify_connection_password("correct horse")), "unlock_throttled");
}

#[test]
fn retry_time_is_capped_when_the_clock_went_back() {
    let fx = fixture!();
    fx.engine.set_policy(&locked_policy(&fx.engine, "correct horse")).unwrap();
    let far = serde_json::json!({ "failures": 0, "locked_until": Utc::now() + Duration::days(30) });
    store::set(&fx.engine.db(), "unlock_throttle", &far).unwrap();
    let err = fx.engine.verify_connection_password("correct horse").unwrap_err();
    assert_eq!(err.code, "unlock_throttled");
    assert!(err.extra["retryAfterSecs"].as_i64().unwrap() <= 300);
}

#[test]
fn an_unparsable_hash_is_unavailable_and_not_counted() {
    let fx = fixture!();
    let mut p = locked_policy(&fx.engine, "x");
    p.connection_password_hash = "not-a-phc-string".into();
    fx.engine.set_policy(&p).unwrap();
    for _ in 0..6 {
        assert_eq!(code(fx.engine.verify_connection_password("x")), "unlock_unavailable");
    }
    assert!(!fx.engine.get_connection().unwrap().unlockable);
}

#[test]
fn language_defaults_to_system() {
    let fx = fixture!();
    assert_eq!(fx.engine.get_settings().unwrap().language, Language::System);
    let next = fx.engine.update_settings(serde_json::json!({ "language": "id" })).unwrap();
    assert_eq!(next.language, Language::Id);
    assert_eq!(fx.engine.get_settings().unwrap().language, Language::Id);
}

#[test]
fn locked_change_needs_password() {
    let fx = fixture!();
    assert!(fx.engine.authorize_connection_change(None).is_ok(), "editable ignores the password");
    fx.engine.set_policy(&locked_policy(&fx.engine, "correct horse")).unwrap();
    assert_eq!(code(fx.engine.authorize_connection_change(None)), "connection_locked");
    assert_eq!(code(fx.engine.authorize_connection_change(Some("bad"))), "wrong_password");
    fx.engine.authorize_connection_change(Some("correct horse")).unwrap();
}
