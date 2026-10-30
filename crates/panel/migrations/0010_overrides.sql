-- Override di PHP e Caddy, per sito (site_id) o globali (NULL). Ogni salvataggio è una versione:
-- quella attiva è l'ultima, il ripristino ne crea una nuova con il contenuto di una vecchia.
CREATE TABLE config_overrides (
    id INTEGER PRIMARY KEY,
    site_id INTEGER REFERENCES sites(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('php', 'caddy')),
    content TEXT NOT NULL,
    version INTEGER NOT NULL,
    author_id INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE UNIQUE INDEX override_versions ON config_overrides (COALESCE(site_id, 0), kind, version);
