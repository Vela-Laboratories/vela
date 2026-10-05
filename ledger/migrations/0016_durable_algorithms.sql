CREATE TABLE api_algo_parents (
    id BIGSERIAL PRIMARY KEY,
    namespace TEXT NOT NULL,
    owner TEXT NOT NULL CHECK (owner ~ '^0x[0-9a-f]{40}$'),
    signer TEXT NOT NULL CHECK (signer ~ '^0x[0-9a-f]{40}$'),
    request JSONB NOT NULL,
    quantity_tick NUMERIC(20,0) NOT NULL CHECK (quantity_tick > 0),
    started_at_ms NUMERIC(20,0) NOT NULL CHECK (started_at_ms >= 0),
    nonce NUMERIC(20,0) NOT NULL CHECK (nonce >= 0),
    nonce_end NUMERIC(20,0) NOT NULL CHECK (nonce_end >= nonce AND nonce_end <= 18446744073709551615),
    filled_quantity NUMERIC(20,0) NOT NULL DEFAULT 0 CHECK (filled_quantity >= 0),
    next_slice INTEGER NOT NULL DEFAULT 0 CHECK (next_slice BETWEEN 0 AND 240),
    cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL DEFAULT 'Running' CHECK (status IN ('Running','Completed','Incomplete','Canceled','Failed','RecoveryRequired')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(namespace,owner,nonce)
);
CREATE INDEX api_algo_owner ON api_algo_parents(namespace,owner,id);
CREATE TABLE api_algo_children (
    parent_id BIGINT NOT NULL REFERENCES api_algo_parents(id),
    slice_index INTEGER NOT NULL CHECK (slice_index BETWEEN 0 AND 239),
    client_order_id TEXT NOT NULL UNIQUE,
    quantity NUMERIC(20,0) NOT NULL CHECK (quantity > 0),
    state TEXT NOT NULL CHECK (state IN ('started','complete','indeterminate')),
    filled_quantity NUMERIC(20,0) CHECK (filled_quantity >= 0 AND filled_quantity <= quantity),
    response_hash BYTEA CHECK (octet_length(response_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(parent_id,slice_index),
    CHECK ((state = 'complete') = (filled_quantity IS NOT NULL))
);
CREATE TABLE api_algo_events (
    id BIGSERIAL PRIMARY KEY,
    parent_id BIGINT NOT NULL REFERENCES api_algo_parents(id),
    action TEXT NOT NULL,
    detail JSONB NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE FUNCTION api_algo_immutable_event() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'algorithm events are immutable'; END $$;
CREATE TRIGGER api_algo_event_guard BEFORE UPDATE OR DELETE ON api_algo_events
FOR EACH ROW EXECUTE FUNCTION api_algo_immutable_event();
CREATE FUNCTION api_algo_parent_identity_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.id,NEW.namespace,NEW.owner,NEW.signer,NEW.request,NEW.quantity_tick,NEW.started_at_ms,NEW.nonce,NEW.nonce_end)
       IS DISTINCT FROM (OLD.id,OLD.namespace,OLD.owner,OLD.signer,OLD.request,OLD.quantity_tick,OLD.started_at_ms,OLD.nonce,OLD.nonce_end) THEN
        RAISE EXCEPTION 'algorithm parent authorization is immutable';
    END IF;
    IF NEW.next_slice < OLD.next_slice OR NEW.filled_quantity < OLD.filled_quantity OR (OLD.cancel_requested AND NOT NEW.cancel_requested) THEN
        RAISE EXCEPTION 'algorithm progress cannot regress';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER api_algo_parent_guard BEFORE UPDATE ON api_algo_parents
FOR EACH ROW EXECUTE FUNCTION api_algo_parent_identity_guard();
CREATE FUNCTION api_algo_child_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.parent_id,NEW.slice_index,NEW.client_order_id,NEW.quantity) IS DISTINCT FROM (OLD.parent_id,OLD.slice_index,OLD.client_order_id,OLD.quantity) THEN
        RAISE EXCEPTION 'algorithm child intent is immutable';
    END IF;
    IF OLD.state = 'complete' AND (NEW.state,NEW.filled_quantity,NEW.response_hash) IS DISTINCT FROM (OLD.state,OLD.filled_quantity,OLD.response_hash) THEN
        RAISE EXCEPTION 'completed algorithm child cannot change';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER api_algo_child_identity_guard BEFORE UPDATE ON api_algo_children
FOR EACH ROW EXECUTE FUNCTION api_algo_child_guard();
