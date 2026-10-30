-- Worker long-running (eseguiti nel container php-workers) e deploy git delle app.
CREATE TABLE workers (
    id INTEGER PRIMARY KEY,
    site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    processes INTEGER NOT NULL DEFAULT 1,
    memory_mb INTEGER NOT NULL DEFAULT 256,
    active INTEGER NOT NULL DEFAULT 1,
    UNIQUE (site_id, name)
);
ALTER TABLE sites ADD COLUMN deploy_repo TEXT;
ALTER TABLE sites ADD COLUMN deploy_branch TEXT;
