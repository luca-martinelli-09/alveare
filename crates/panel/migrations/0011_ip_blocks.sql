-- Blocchi di IP decisi dal pannello: site_id NULL vale per tutti i siti. expires_at NULL = per sempre.
CREATE TABLE ip_blocks (
    id INTEGER PRIMARY KEY,
    site_id INTEGER REFERENCES sites(id) ON DELETE CASCADE,
    cidr TEXT NOT NULL,
    expires_at INTEGER,
    created_by INTEGER,
    created_at INTEGER NOT NULL
);
