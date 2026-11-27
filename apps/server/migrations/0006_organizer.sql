-- The person's synced collections and project sets (`typeshelf_core::organizer::OrganizerDoc` as JSON), one row
-- per install that pushed. A push stores the group's merge under the person and deletes the group's other rows.
-- No foreign key: `free_revoked_id` moves the row with the rest of a revoked install's history.
CREATE TABLE organizer_docs (
    install_id TEXT PRIMARY KEY,
    rev INTEGER NOT NULL,
    doc_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
