-- Spazi: "siti" senza pagine pubbliche (script, cron, file), mai su Caddy, sul tunnel o nel monitoraggio.
ALTER TABLE sites ADD COLUMN space INTEGER NOT NULL DEFAULT 0;
