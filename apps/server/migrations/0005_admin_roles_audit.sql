-- Roles: 'owner' | 'admin'. A database from before roles were checked gets its earliest admin as the owner.
UPDATE admins SET role = 'owner'
WHERE id = (SELECT MIN(id) FROM admins) AND NOT EXISTS (SELECT 1 FROM admins WHERE role = 'owner');

-- Who did what, to what, when. `admin` is the email (kept when the admin is removed; 'command line' for the CLI).
-- Never holds passwords, tokens or the install key.
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY,
    at TEXT NOT NULL,
    admin TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    detail TEXT NOT NULL
);
CREATE INDEX audit_log_at ON audit_log(at);
