-- Link monouso via email: invito (72 ore) e reset della password (1 ora). Solo l'hash del token.
CREATE TABLE user_links (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose TEXT NOT NULL CHECK (purpose IN ('invite', 'reset')),
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
