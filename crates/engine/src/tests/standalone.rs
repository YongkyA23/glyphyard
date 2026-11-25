//! Standalone mode: a name alone finishes onboarding; the server URL says whether there is a server at all.

use super::*;

#[test]
fn onboarded_locally_without_a_server() {
    let fx = fixture!();
    let state = fx.engine.get_app_state().unwrap();
    assert!(!state.onboarded && !state.server_configured, "a fresh install with no server");

    // No server, no token: the name alone is enough.
    fx.engine.set_identity("Ana", None).unwrap();
    let state = fx.engine.get_app_state().unwrap();
    assert!(state.onboarded);
    assert!(fx.engine.token().unwrap().is_none());
    assert_eq!(serde_json::to_value(&state).unwrap()["serverConfigured"], false);

    // A server added later (Settings › Connection) keeps the person onboarded; the sync loop registers with the saved name.
    fx.engine.set_connection("https://fonts.example", "key").unwrap();
    let state = fx.engine.get_app_state().unwrap();
    assert!(state.onboarded && state.server_configured);
    assert_eq!(state.name, "Ana");

    // Clearing the address (spaces count as empty) is standalone again.
    fx.engine.set_connection("  ", "").unwrap();
    assert!(!fx.engine.get_app_state().unwrap().server_configured);
}
