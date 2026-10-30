-- Avvisi mandati su Telegram: all'apertura e alla risoluzione, una volta sola.
ALTER TABLE alerts ADD COLUMN notified INTEGER NOT NULL DEFAULT 0;
ALTER TABLE alerts ADD COLUMN resolved_notified INTEGER NOT NULL DEFAULT 0;
