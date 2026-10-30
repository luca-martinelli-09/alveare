-- Esportazioni scaricabili per 7 giorni: il token è nel link, il file in data/exports.
CREATE TABLE exports (
    token TEXT PRIMARY KEY,
    site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    file TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
