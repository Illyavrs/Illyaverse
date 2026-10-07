-- Reversible availability is independent of ranking status and immutable bytes.
-- Official sets may be withheld before any difficulty has been cached, so this
-- authority deliberately has no FK to a particular beatmap or package row.
CREATE TABLE beatmapset_availability (
    native_set_id BIGINT PRIMARY KEY CHECK(native_set_id BETWEEN 1 AND 2147483647),
    withdrawn BOOLEAN NOT NULL,
    revision BIGINT NOT NULL CHECK(revision >= 1),
    reason TEXT NOT NULL CHECK(reason = btrim(reason) AND char_length(reason) BETWEEN 1 AND 500 AND octet_length(reason) <= 2000),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    actor_id BIGINT NOT NULL REFERENCES users
);

-- A missing state means active with revision zero. Keep this read predicate
-- stable for coherent catalogue/ranking snapshots; admission below is volatile.
CREATE FUNCTION illya_beatmapset_active(set_id BIGINT) RETURNS BOOLEAN
LANGUAGE sql STABLE STRICT AS $$
    SELECT set_id BETWEEN 1 AND 2147483647 AND NOT EXISTS(
        SELECT 1 FROM beatmapset_availability a
        WHERE a.native_set_id=set_id AND a.withdrawn
    )
$$;
CREATE VIEW active_beatmaps AS
    SELECT b.* FROM beatmaps b WHERE illya_beatmapset_active(b.native_set_id);

-- Shared admission and exclusive staff/publication fences use the existing
-- official-set namespace, extended to every canonical native set identity.
-- Take authority first, then distinct ascending set IDs, then aliases/checksums
-- and row resources. Never hold this fence across an upstream HTTP request.
CREATE FUNCTION illya_lock_beatmapset(set_id BIGINT) RETURNS VOID
LANGUAGE plpgsql VOLATILE STRICT AS $$
BEGIN
    IF set_id NOT BETWEEN 1 AND 2147483647 THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid canonical beatmap set identity';
    END IF;
    PERFORM pg_advisory_xact_lock_shared(1229737811,set_id::INTEGER);
END $$;
CREATE FUNCTION illya_lock_active_beatmapset(set_id BIGINT) RETURNS VOID
LANGUAGE plpgsql VOLATILE STRICT AS $$
BEGIN
    PERFORM illya_lock_beatmapset(set_id);
    -- This separate statement observes a withdrawal committed during the wait.
    IF NOT illya_beatmapset_active(set_id) THEN
        RAISE EXCEPTION USING ERRCODE='P2001',MESSAGE='beatmap_withdrawn';
    END IF;
END $$;

-- The API takes the exclusive set fence BEFORE touching the state row. Do not
-- acquire it in this row trigger: UPDATE has already locked its row, which
-- would invert the API's advisory-before-row order. Restored tombstones remain.
CREATE FUNCTION illya_preserve_beatmapset_availability() RETURNS trigger
LANGUAGE plpgsql VOLATILE AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='beatmap set availability history is retained';
    ELSIF TG_OP='INSERT' THEN
        IF NOT NEW.withdrawn OR NEW.revision<>1 THEN
            RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='initial availability transition must withdraw revision one';
        END IF;
    ELSIF NEW.native_set_id IS DISTINCT FROM OLD.native_set_id
        OR NEW.withdrawn IS NOT DISTINCT FROM OLD.withdrawn
        OR NEW.revision<>OLD.revision+1 THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='availability transitions preserve identity and advance revision';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER beatmapset_availability_identity BEFORE INSERT OR UPDATE OR DELETE
    ON beatmapset_availability FOR EACH ROW EXECUTE FUNCTION illya_preserve_beatmapset_availability();
CREATE FUNCTION lazer_record_beatmapset_availability() RETURNS trigger
LANGUAGE plpgsql VOLATILE AS $$
BEGIN
    INSERT INTO lazer_beatmap_updates(beatmap_set_id) VALUES(NEW.native_set_id);
    RETURN NEW;
END $$;
CREATE TRIGGER beatmapset_availability_event AFTER INSERT OR UPDATE
    ON beatmapset_availability FOR EACH ROW EXECUTE FUNCTION lazer_record_beatmapset_availability();

CREATE FUNCTION illya_guard_new_beatmap_content() RETURNS trigger
LANGUAGE plpgsql VOLATILE AS $$
BEGIN
    IF TG_TABLE_NAME='official_beatmapset_archives' THEN
        PERFORM illya_lock_active_beatmapset(NEW.official_set_id);
    ELSE
        PERFORM illya_lock_active_beatmapset(NEW.native_set_id);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER beatmap_packages_withdrawal_guard BEFORE INSERT ON beatmap_packages
    FOR EACH ROW EXECUTE FUNCTION illya_guard_new_beatmap_content();
-- PostgreSQL orders equivalent triggers by name: bind immutable native identity
-- first, then guard publication before insertion can lock conflicting map rows.
CREATE TRIGGER beatmaps_withdrawal_guard BEFORE INSERT ON beatmaps
    FOR EACH ROW EXECUTE FUNCTION illya_guard_new_beatmap_content();
CREATE TRIGGER official_archives_withdrawal_guard BEFORE INSERT ON official_beatmapset_archives
    FOR EACH ROW EXECUTE FUNCTION illya_guard_new_beatmap_content();

CREATE FUNCTION illya_guard_new_beatmap_score() RETURNS trigger
LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    set_id BIGINT;
BEGIN
    -- BEFORE INSERT also runs for ON CONFLICT retries. A persisted exact key
    -- identifies retained publication, not permission to create another result.
    IF NEW.ingestion_key IS NOT NULL AND EXISTS(
        SELECT 1 FROM scores s WHERE s.ingestion_key=NEW.ingestion_key
        AND s.user_id=NEW.user_id AND s.beatmap_id=NEW.beatmap_id
        AND s.mode=NEW.mode AND s.score_variant=NEW.score_variant
    ) THEN
        RETURN NEW;
    END IF;
    SELECT b.native_set_id INTO set_id FROM beatmaps b WHERE b.id=NEW.beatmap_id;
    IF NOT FOUND THEN
        -- Leave the ordinary score FK to report a missing immutable map row.
        RETURN NEW;
    END IF;
    PERFORM illya_lock_beatmapset(set_id);
    -- A concurrently saved exact retry may have committed while this waited.
    IF NEW.ingestion_key IS NOT NULL AND EXISTS(
        SELECT 1 FROM scores s WHERE s.ingestion_key=NEW.ingestion_key
        AND s.user_id=NEW.user_id AND s.beatmap_id=NEW.beatmap_id
        AND s.mode=NEW.mode AND s.score_variant=NEW.score_variant
    ) THEN
        RETURN NEW;
    END IF;
    IF NOT illya_beatmapset_active(set_id) THEN
        RAISE EXCEPTION USING ERRCODE='P2001',MESSAGE='beatmap_withdrawn';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER scores_withdrawal_guard BEFORE INSERT ON scores
    FOR EACH ROW EXECUTE FUNCTION illya_guard_new_beatmap_score();
