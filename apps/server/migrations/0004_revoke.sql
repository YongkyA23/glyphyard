-- Set when an admin removes the computer: its token is refused and it leaves every list; its history stays.
ALTER TABLE installs ADD COLUMN revoked_at TEXT;
