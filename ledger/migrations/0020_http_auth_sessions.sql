-- Shared wallet auth state. Store token hashes only; never persist bearer tokens.
CREATE TABLE api_http_auth_challenges (
    nonce TEXT PRIMARY KEY CHECK (nonce ~ '^[0-9a-f]{32}$'),
    scope TEXT NOT NULL CHECK (length(scope) BETWEEN 1 AND 64),
    address TEXT NOT NULL CHECK (address ~ '^0x[0-9a-f]{40}$'),
    expires_at_ms BIGINT NOT NULL CHECK (expires_at_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE api_http_auth_sessions (
    token_hash BYTEA PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    scope TEXT NOT NULL CHECK (length(scope) BETWEEN 1 AND 64),
    address TEXT NOT NULL CHECK (address ~ '^0x[0-9a-f]{40}$'),
    expires_at_ms BIGINT NOT NULL CHECK (expires_at_ms > 0),
    revoked_at_ms BIGINT CHECK (revoked_at_ms IS NULL OR revoked_at_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE INDEX api_http_auth_sessions_expiry_idx
    ON api_http_auth_sessions (expires_at_ms);
CREATE INDEX api_http_auth_challenges_expiry_idx
    ON api_http_auth_challenges (expires_at_ms);
