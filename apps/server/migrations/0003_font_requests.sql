-- "I need this font" requests from desktops. `install_id` is the install that asked; done_at NULL = open.
CREATE TABLE font_requests (
    id INTEGER PRIMARY KEY,
    install_id TEXT NOT NULL REFERENCES installs(id),
    font_name TEXT NOT NULL,
    note TEXT,
    at TEXT NOT NULL,
    done_at TEXT
);
CREATE INDEX font_requests_install ON font_requests(install_id, done_at);
CREATE INDEX font_requests_at ON font_requests(at);
