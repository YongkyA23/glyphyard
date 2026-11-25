//! Asking the admin for a font: checked here, sent as an event through the outbox.

use super::*;

#[test]
fn a_font_request_is_trimmed_and_sent_as_an_event() {
    let fx = fixture!();
    fx.engine.request_font("  Söhne Buch ", Some("  for the Acme deck ")).unwrap();
    fx.engine.request_font("Inter", Some("   ")).unwrap();
    let events = fx.events.lock().unwrap().clone();
    assert_eq!(
        events,
        [
            Event::FontRequested { name: "Söhne Buch".into(), note: Some("for the Acme deck".into()) },
            Event::FontRequested { name: "Inter".into(), note: None },
        ]
    );
}

#[test]
fn a_font_request_needs_a_name_and_stays_short() {
    let fx = fixture!();
    for (name, note) in [("   ", None), (&*"x".repeat(201), None), ("Inter", Some(&*"n".repeat(1001)))] {
        let err = fx.engine.request_font(name, note).unwrap_err();
        assert_eq!(err.code, "invalid_request");
    }
    // The limits count characters, not bytes.
    fx.engine.request_font(&"é".repeat(200), Some(&"é".repeat(1000))).unwrap();
    assert_eq!(fx.events.lock().unwrap().len(), 1);
}
