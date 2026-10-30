-- Storico dei controlli di raggiungibilità (ogni 5 minuti, 30 giorni) per il grafico del sito.
CREATE TABLE uptime_samples (
    site_id INTEGER NOT NULL,
    at INTEGER NOT NULL,
    ms INTEGER NOT NULL,
    ok INTEGER NOT NULL
);
CREATE INDEX uptime_samples_site_at ON uptime_samples (site_id, at);
