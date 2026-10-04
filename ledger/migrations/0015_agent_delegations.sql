-- Namespace keeps each runtime/chain's testnet permissions independent.
CREATE TABLE api_agent_delegations (
    namespace TEXT NOT NULL,
    agent TEXT NOT NULL CHECK (agent ~ '^0x[0-9a-f]{40}$'),
    master TEXT NOT NULL CHECK (master ~ '^0x[0-9a-f]{40}$'),
    expires_at_ms NUMERIC(20, 0) NOT NULL CHECK (expires_at_ms BETWEEN 0 AND 18446744073709551615),
    max_notional_per_order NUMERIC(20, 0) NOT NULL CHECK (max_notional_per_order BETWEEN 1 AND 18446744073709551615),
    nonce NUMERIC(20, 0) NOT NULL CHECK (nonce BETWEEN 0 AND 18446744073709551615),
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    scope JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (namespace, agent)
);
CREATE INDEX api_agent_master ON api_agent_delegations(namespace, master);

-- Signed requests are retained as immutable evidence, including revocations.
CREATE TABLE api_agent_events (
    namespace TEXT NOT NULL,
    agent TEXT NOT NULL,
    master TEXT NOT NULL,
    nonce NUMERIC(20, 0) NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('register', 'revoke')),
    message TEXT NOT NULL,
    signature TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (namespace, agent, nonce)
);

-- Counters use UTC hour/day buckets and are shared by all API instances.
CREATE TABLE api_agent_notional_buckets (
    namespace TEXT NOT NULL,
    agent TEXT NOT NULL,
    bucket_seconds BIGINT NOT NULL CHECK (bucket_seconds IN (3600, 86400)),
    bucket_id NUMERIC(20, 0) NOT NULL,
    notional NUMERIC(40, 0) NOT NULL CHECK (notional >= 0),
    PRIMARY KEY (namespace, agent, bucket_seconds, bucket_id)
);
