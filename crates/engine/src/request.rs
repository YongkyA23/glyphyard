//! "I need this font": a request to the admin, sent as an event through the sync outbox (so it waits while offline).

use crate::{Engine, EngineError, Result};
use typeshelf_protocol::{Event, FONT_REQUEST_NAME_MAX, FONT_REQUEST_NOTE_MAX};

impl Engine {
    /// Queues a request for `name` with an optional note. Both are trimmed; an empty note is no note.
    pub fn request_font(&self, name: &str, note: Option<&str>) -> Result<()> {
        let name = name.trim();
        let note = note.map(str::trim).filter(|n| !n.is_empty());
        if name.is_empty() {
            return Err(EngineError::new("invalid_request", "Type the name of the font you need."));
        }
        if name.chars().count() > FONT_REQUEST_NAME_MAX {
            return Err(EngineError::new("invalid_request", format!("The font name can be at most {FONT_REQUEST_NAME_MAX} characters.")));
        }
        if note.is_some_and(|n| n.chars().count() > FONT_REQUEST_NOTE_MAX) {
            return Err(EngineError::new("invalid_request", format!("The note can be at most {FONT_REQUEST_NOTE_MAX} characters.")));
        }
        self.emit(Event::FontRequested { name: name.to_string(), note: note.map(String::from) });
        Ok(())
    }
}
