//! Task 10: the "Playful extras" setting.

use super::*;

#[test]
fn playful_defaults_on_and_can_be_turned_off() {
    let fx = fixture!();
    assert!(fx.engine.get_settings().unwrap().playful);
    store::set(&fx.engine.db(), "app", &serde_json::json!({ "sampleText": "old" })).unwrap();
    assert!(fx.engine.get_settings().unwrap().playful, "older stored settings default to on");
    let s = fx.engine.update_settings(serde_json::json!({ "playful": false })).unwrap();
    assert!(!s.playful);
    assert!(!fx.engine.get_settings().unwrap().playful);
}
