-- Account Cloudflare (etichetta del .env) che serve il sito; NULL = il primo configurato.
ALTER TABLE sites ADD COLUMN cloudflare_account TEXT;
