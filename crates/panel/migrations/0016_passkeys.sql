-- Passkey (WebAuthn): accesso senza password, con verifica dell'utente sul dispositivo.
ALTER TABLE users ADD COLUMN webauthn_id TEXT;
CREATE TABLE passkeys (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    cred_id TEXT NOT NULL UNIQUE,
    data TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used INTEGER
);
