-- AUTOINCREMENT: un id (e quindi un UID Unix) non viene mai riusato, anche dopo l'eliminazione.
CREATE TABLE sites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    slug TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('wordpress', 'php', 'laravel')),
    template TEXT NOT NULL,
    php_version TEXT NOT NULL DEFAULT '8.5',
    status TEXT NOT NULL DEFAULT 'creating' CHECK (status IN ('creating', 'active', 'suspended', 'error', 'deleting')),
    staging INTEGER NOT NULL DEFAULT 0,
    update_rule TEXT NOT NULL DEFAULT 'security' CHECK (update_rule IN ('auto', 'security', 'manual')),
    memory_limit_mb INTEGER NOT NULL DEFAULT 256,
    upload_max_mb INTEGER NOT NULL DEFAULT 64,
    max_execution_time_s INTEGER NOT NULL DEFAULT 60,
    max_children INTEGER NOT NULL DEFAULT 5,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE domains (
    id INTEGER PRIMARY KEY,
    site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    hostname TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('primary', 'alias', 'redirect'))
);
CREATE UNIQUE INDEX one_primary_per_site ON domains(site_id) WHERE kind = 'primary';

CREATE TABLE site_databases (
    site_id INTEGER PRIMARY KEY REFERENCES sites(id) ON DELETE CASCADE,
    db_name TEXT NOT NULL UNIQUE,
    db_user TEXT NOT NULL UNIQUE,
    password_enc BLOB NOT NULL
);

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    site_id INTEGER,
    user_id INTEGER,
    status TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'done', 'failed')),
    log TEXT NOT NULL DEFAULT '',
    started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    finished_at TEXT
);
