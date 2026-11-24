-- Rust-owned server-side sessions (tower-sessions). The Go oracle keeps using
-- the legacy `sessions` table. The two are intentionally separate.
CREATE TABLE IF NOT EXISTS web_sessions (
    id TEXT PRIMARY KEY,
    data BLOB NOT NULL,
    expiry INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS web_sessions_expiry ON web_sessions(expiry);
