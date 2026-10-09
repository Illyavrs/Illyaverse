-- Keep historical statuses and the legacy INSERT default unchanged. New local
-- uploads explicitly choose graveyard at their API boundary.
ALTER TABLE beatmaps DROP CONSTRAINT beatmaps_status_check;
ALTER TABLE beatmaps ADD CONSTRAINT beatmaps_status_check
    CHECK (status IN ('pending','ranked','loved','unranked','graveyard','wip'));
ALTER TABLE beatmaps ADD COLUMN status_revision BIGINT NOT NULL DEFAULT 0
    CHECK (status_revision >= 0);

-- Official difficulties can receive a decision before their content has been
-- imported, so this authority has no FK to a particular cached map row. Each
-- difficulty owns its status; the set ID records its verified identity context.
CREATE TABLE beatmap_status_overrides (
    native_id BIGINT PRIMARY KEY CHECK (native_id BETWEEN 1 AND 2147483647),
    native_set_id BIGINT NOT NULL CHECK (native_set_id BETWEEN 1 AND 2147483647),
    status TEXT NOT NULL CHECK (status IN ('pending','ranked','loved','unranked','graveyard','wip')),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision >= 1),
    actor_id BIGINT NOT NULL REFERENCES users,
    reason TEXT NOT NULL CHECK (reason = btrim(reason)
        AND char_length(reason) BETWEEN 1 AND 1000 AND octet_length(reason) <= 4000
        AND reason !~ '[[:cntrl:]]'),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE FUNCTION illya_preserve_beatmap_status_override() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    existing_set_id BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING ERRCODE = '23514',
            MESSAGE = 'beatmap difficulty status decisions must be changed explicitly';
    ELSIF TG_OP = 'INSERT' THEN
        NEW.revision := 1;
        NEW.changed_at := clock_timestamp();
    ELSE
        IF NEW.native_id IS DISTINCT FROM OLD.native_id
            OR NEW.native_set_id IS DISTINCT FROM OLD.native_set_id THEN
            RAISE EXCEPTION USING ERRCODE = '23514',
                MESSAGE = 'beatmap difficulty status decision identity is immutable';
        END IF;
        NEW.revision := OLD.revision + CASE
            WHEN NEW.status IS DISTINCT FROM OLD.status THEN 1 ELSE 0 END;
        IF NEW.status IS DISTINCT FROM OLD.status
            OR NEW.actor_id IS DISTINCT FROM OLD.actor_id
            OR NEW.reason IS DISTINCT FROM OLD.reason THEN
            NEW.changed_at := clock_timestamp();
        ELSE
            NEW.changed_at := OLD.changed_at;
        END IF;
    END IF;
    SELECT native_set_id INTO existing_set_id FROM beatmaps WHERE native_id = NEW.native_id;
    IF FOUND THEN
        IF existing_set_id IS DISTINCT FROM NEW.native_set_id THEN
            RAISE EXCEPTION USING ERRCODE = '23514',
                MESSAGE = 'beatmap difficulty status decision has a different set identity';
        END IF;
    ELSIF NEW.native_id >= 2000000000 OR NEW.native_set_id >= 2000000000 THEN
        RAISE EXCEPTION USING ERRCODE = '23514',
            MESSAGE = 'local difficulty status decisions require a stored canonical map';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER beatmap_status_override_identity
    BEFORE INSERT OR UPDATE OR DELETE ON beatmap_status_overrides FOR EACH ROW
    EXECUTE FUNCTION illya_preserve_beatmap_status_override();

CREATE FUNCTION illya_apply_beatmap_status_override() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    server_status TEXT;
    server_set_id BIGINT;
BEGIN
    SELECT status, native_set_id INTO server_status, server_set_id FROM beatmap_status_overrides
        WHERE native_id = COALESCE(NEW.official_id, NEW.local_native_id);
    IF FOUND THEN
        IF server_set_id IS DISTINCT FROM NEW.native_set_id THEN
            RAISE EXCEPTION USING ERRCODE = '23514',
                MESSAGE = 'beatmap difficulty does not match its server status decision';
        END IF;
        NEW.status := server_status;
    END IF;
    RETURN NEW;
END;
$$;
-- PostgreSQL orders same-event triggers by name. Read the override after native
-- binding and the withdrawal guard's shared set fence, avoiding a stale status
-- read before a concurrent staff decision finishes. UPDATE writers acquire the
-- set fence before taking map row locks; this trigger must not invert that order.
CREATE TRIGGER beatmaps_zz_server_status_override
    BEFORE INSERT OR UPDATE ON beatmaps FOR EACH ROW
    EXECUTE FUNCTION illya_apply_beatmap_status_override();

CREATE FUNCTION illya_advance_beatmap_status_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.status_revision := 0;
    ELSIF NEW.status IS DISTINCT FROM OLD.status THEN
        NEW.status_revision := OLD.status_revision + 1;
    ELSE
        NEW.status_revision := OLD.status_revision;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER beatmaps_zzz_status_revision
    BEFORE INSERT OR UPDATE ON beatmaps FOR EACH ROW
    EXECUTE FUNCTION illya_advance_beatmap_status_revision();

-- SELECT * in a stored view does not automatically expose later table columns.
CREATE OR REPLACE VIEW active_beatmaps AS
    SELECT b.* FROM beatmaps b WHERE illya_beatmapset_active(b.native_set_id);

CREATE FUNCTION illya_valid_rank_difficulty_ids(value BIGINT[]) RETURNS BOOLEAN
LANGUAGE sql IMMUTABLE STRICT AS $$
    SELECT cardinality(value) BETWEEN 1 AND 512
        AND array_position(value, NULL) IS NULL
        AND cardinality(value) = (SELECT count(DISTINCT entry) FROM unnest(value) entry)
        AND (SELECT bool_and(entry BETWEEN 1 AND 2147483647) FROM unnest(value) entry)
$$;

CREATE TABLE beatmap_rank_requests (
    id BIGSERIAL PRIMARY KEY,
    native_set_id BIGINT NOT NULL CHECK (native_set_id BETWEEN 1 AND 2147483647),
    source TEXT NOT NULL CHECK (source IN ('local','official')),
    requester_id BIGINT NOT NULL REFERENCES users,
    reason TEXT NOT NULL CHECK (reason = btrim(reason)
        AND char_length(reason) BETWEEN 1 AND 1000 AND octet_length(reason) <= 4000
        AND reason !~ '[[:cntrl:]]'),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','approved','rejected','withdrawn')),
    revision BIGINT NOT NULL DEFAULT 0 CHECK (revision >= 0),
    metadata JSONB NOT NULL CHECK (jsonb_typeof(metadata) = 'object'),
    review_reason TEXT CHECK (review_reason = btrim(review_reason)
        AND char_length(review_reason) BETWEEN 1 AND 1000
        AND octet_length(review_reason) <= 4000 AND review_reason !~ '[[:cntrl:]]'),
    reviewed_by BIGINT REFERENCES users,
    approved_difficulty_ids BIGINT[] CHECK (illya_valid_rank_difficulty_ids(approved_difficulty_ids)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    reviewed_at TIMESTAMPTZ,
    CHECK ((status = 'approved') = (approved_difficulty_ids IS NOT NULL))
);
CREATE UNIQUE INDEX beatmap_rank_requests_pending_set ON beatmap_rank_requests(native_set_id)
    WHERE status = 'pending';
CREATE INDEX beatmap_rank_requests_requester ON beatmap_rank_requests(requester_id,id DESC);
CREATE INDEX beatmap_rank_requests_status ON beatmap_rank_requests(status,id DESC);

CREATE FUNCTION illya_preserve_beatmap_rank_request() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.revision := 0;
        NEW.created_at := clock_timestamp();
        NEW.updated_at := NEW.created_at;
    ELSE
        IF NEW.id IS DISTINCT FROM OLD.id
            OR NEW.native_set_id IS DISTINCT FROM OLD.native_set_id
            OR NEW.source IS DISTINCT FROM OLD.source
            OR NEW.requester_id IS DISTINCT FROM OLD.requester_id
            OR NEW.metadata IS DISTINCT FROM OLD.metadata
            OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
            RAISE EXCEPTION USING ERRCODE = '23514',
                MESSAGE = 'beatmap rank request identity and submission snapshot are immutable';
        END IF;
        IF NEW.status IS DISTINCT FROM OLD.status
            OR NEW.reason IS DISTINCT FROM OLD.reason
            OR NEW.review_reason IS DISTINCT FROM OLD.review_reason
            OR NEW.reviewed_by IS DISTINCT FROM OLD.reviewed_by
            OR NEW.approved_difficulty_ids IS DISTINCT FROM OLD.approved_difficulty_ids
            OR NEW.reviewed_at IS DISTINCT FROM OLD.reviewed_at THEN
            NEW.revision := OLD.revision + 1;
            NEW.updated_at := clock_timestamp();
        ELSE
            NEW.revision := OLD.revision;
            NEW.updated_at := OLD.updated_at;
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER beatmap_rank_requests_identity
    BEFORE INSERT OR UPDATE ON beatmap_rank_requests FOR EACH ROW
    EXECUTE FUNCTION illya_preserve_beatmap_rank_request();

-- Existing custom roles remain explicitly configured. Never replace a role's
-- current grants or silently elevate a pre-existing role with the new BN name.
UPDATE roles SET permissions = array_append(permissions, 'beatmaps.rank.request')
    WHERE name IN ('player','moderator')
        AND NOT ('beatmaps.rank.request' = ANY(permissions));
UPDATE roles SET permissions = array_append(permissions, 'beatmaps.rank.review')
    WHERE name = 'moderator' AND NOT ('beatmaps.rank.review' = ANY(permissions));
INSERT INTO roles(name,description,permissions)
    VALUES ('beatmap_nominator','Beatmap nomination and ranking review',
        ARRAY['chat.send','beatmaps.upload','beatmaps.import','beatmaps.moderate',
            'beatmaps.rank.request','beatmaps.rank.review'])
    ON CONFLICT (name) DO NOTHING;
