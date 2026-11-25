//! Commands fanned out to a merged group: a computer without the font leaves the report to the ack.

use super::*;

#[test]
fn admin_activate_of_a_font_not_here_fails_without_its_own_event() {
    let fx = fixture!();
    let missing = FontRef { hash: "nope".into(), ps_name: "Nope-Regular".into(), family: "Nope".into(), style: "Regular".into() };
    // The ack text the server's matcher recognises (`hub::font_not_here`).
    let err = fx.engine.apply_command(&CommandKind::Activate { font: missing }).unwrap_err().to_string();
    assert_eq!(err, format!("{}: Nope-Regular{}", typeshelf_protocol::FONT_NOT_HERE_CODE, typeshelf_protocol::FONT_NOT_HERE_SUFFIX));
    assert!(!fx.events.lock().unwrap().iter().any(|e| matches!(e, Event::ActivationFailed { .. })), "the ack is the only report");
}
