CREATE TABLE ledger_engine_leadership (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    owner_id UUID,
    generation BIGINT NOT NULL DEFAULT 0 CHECK (generation >= 0),
    lease_expires_at TIMESTAMPTZ,
    acquired_at TIMESTAMPTZ,
    renewed_at TIMESTAMPTZ,
    CHECK ((owner_id IS NULL) = (lease_expires_at IS NULL))
);

INSERT INTO ledger_engine_leadership (singleton) VALUES (TRUE);

CREATE FUNCTION ledger_guard_engine_leadership_generation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.generation < OLD.generation THEN
        RAISE EXCEPTION 'engine leadership generation cannot regress';
    END IF;
    IF NEW.owner_id IS NOT DISTINCT FROM OLD.owner_id
       AND NEW.generation <> OLD.generation THEN
        RAISE EXCEPTION 'one engine owner cannot change its generation';
    END IF;
    IF NEW.owner_id IS DISTINCT FROM OLD.owner_id
       AND NEW.owner_id IS NOT NULL
       AND NEW.generation <= OLD.generation THEN
        RAISE EXCEPTION 'engine takeover must advance the generation';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_engine_leadership_generation_guard
BEFORE UPDATE ON ledger_engine_leadership
FOR EACH ROW EXECUTE FUNCTION ledger_guard_engine_leadership_generation();
