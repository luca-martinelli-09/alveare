CREATE TABLE backup_policies (
    site_id INTEGER PRIMARY KEY REFERENCES sites(id) ON DELETE CASCADE,
    keep_daily INTEGER NOT NULL DEFAULT 7,
    keep_weekly INTEGER NOT NULL DEFAULT 4,
    keep_monthly INTEGER NOT NULL DEFAULT 6
);

-- Senza FK sul sito: lo storico (e il backup finale) resta anche dopo l'eliminazione.
CREATE TABLE backup_runs (
    id INTEGER PRIMARY KEY,
    site_id INTEGER,
    slug TEXT NOT NULL,
    started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    finished_at TEXT,
    status TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'done', 'failed')),
    snapshot_id TEXT,
    bytes_added INTEGER,
    total_bytes INTEGER,
    error TEXT
);
CREATE INDEX backup_runs_by_site ON backup_runs(site_id, id);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
