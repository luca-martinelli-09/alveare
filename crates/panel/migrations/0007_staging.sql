-- Protezione dello staging: password cifrata (per mostrarla al gestore) e hash bcrypt per Caddy.
ALTER TABLE sites ADD COLUMN staging_password_enc BLOB;
ALTER TABLE sites ADD COLUMN staging_hash TEXT;
