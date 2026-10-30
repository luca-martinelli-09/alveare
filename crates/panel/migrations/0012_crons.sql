-- Cron personalizzati dei siti e le loro ultime 50 esecuzioni.
CREATE TABLE cron_jobs (
    id INTEGER PRIMARY KEY,
    site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    expression TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('wp', 'artisan', 'php', 'http')),
    command TEXT NOT NULL,
    timeout_s INTEGER NOT NULL DEFAULT 600,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE cron_runs (
    id INTEGER PRIMARY KEY,
    cron_id INTEGER NOT NULL REFERENCES cron_jobs(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    finished_at TEXT,
    ok INTEGER,
    output TEXT NOT NULL DEFAULT ''
);
CREATE INDEX cron_runs_by_job ON cron_runs (cron_id, id);
