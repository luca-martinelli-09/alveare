-- Gli access log contengono IP (dati personali): durata di conservazione esplicita per sito.
ALTER TABLE sites ADD COLUMN log_retention_days INTEGER NOT NULL DEFAULT 30 CHECK (log_retention_days BETWEEN 1 AND 365);
