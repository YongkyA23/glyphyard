CREATE TABLE installs (
    id TEXT PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    os TEXT NOT NULL,
    app_version TEXT NOT NULL,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    merged_into TEXT REFERENCES installs(id),
    merged_at TEXT
);

-- by: user | first | merge
CREATE TABLE name_changes (
    id INTEGER PRIMARY KEY,
    install_id TEXT NOT NULL REFERENCES installs(id),
    old_name TEXT,
    new_name TEXT NOT NULL,
    by TEXT NOT NULL,
    at TEXT NOT NULL
);
CREATE INDEX name_changes_install ON name_changes(install_id, at);

CREATE TABLE name_limit_resets (
    id INTEGER PRIMARY KEY,
    install_id TEXT NOT NULL REFERENCES installs(id),
    at TEXT NOT NULL,
    admin_id INTEGER NOT NULL
);

-- An override keeps source = 'admin' with active = 0; admin_at/overridden_at record when each side acted.
CREATE TABLE font_states (
    install_id TEXT NOT NULL REFERENCES installs(id),
    hash TEXT NOT NULL,
    ps_name TEXT NOT NULL,
    family TEXT NOT NULL,
    style TEXT NOT NULL,
    active INTEGER NOT NULL,
    source TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    admin_at TEXT,
    overridden_at TEXT,
    PRIMARY KEY (install_id, hash)
);

-- kind: name | font | override | failure | organize | system; by: user | admin | system
CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    install_id TEXT NOT NULL REFERENCES installs(id),
    kind TEXT NOT NULL,
    by TEXT NOT NULL,
    text TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    at TEXT NOT NULL
);
CREATE INDEX events_install_at ON events(install_id, at);
CREATE INDEX events_at ON events(at);

CREATE TABLE commands (
    id INTEGER PRIMARY KEY,
    install_id TEXT NOT NULL REFERENCES installs(id),
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT,
    acked_at TEXT,
    ok INTEGER,
    error TEXT
);
CREATE INDEX commands_pending ON commands(install_id, acked_at);

CREATE TABLE library_files (
    hash TEXT PRIMARY KEY,
    filename TEXT NOT NULL UNIQUE,
    size INTEGER NOT NULL,
    ps_name TEXT NOT NULL,
    family TEXT NOT NULL,
    style TEXT NOT NULL,
    version TEXT NOT NULL,
    license TEXT NOT NULL DEFAULT 'unknown',
    uploaded_at TEXT NOT NULL,
    blocked INTEGER NOT NULL DEFAULT 0
);

-- target: 'all' or an install id
CREATE TABLE deliveries (
    hash TEXT NOT NULL REFERENCES library_files(hash) ON DELETE CASCADE,
    target TEXT NOT NULL,
    activate INTEGER NOT NULL,
    PRIMARY KEY (hash, target)
);

CREATE TABLE admins (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    last_login TEXT
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    admin_id INTEGER NOT NULL REFERENCES admins(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL
);

-- value is JSON
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
