-- Ultimo passo TOTP accettato: un codice già usato non vale una seconda volta.
ALTER TABLE users ADD COLUMN totp_last_step INTEGER NOT NULL DEFAULT 0;
