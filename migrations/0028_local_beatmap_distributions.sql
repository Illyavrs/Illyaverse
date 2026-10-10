-- Derived client downloads bind canonical local IDs without changing the
-- original upload, checksum, score foreign keys, or historical replay bytes.
CREATE FUNCTION illya_valid_optional_map_manifest(value TEXT[]) RETURNS BOOLEAN
LANGUAGE sql IMMUTABLE STRICT AS $$
    SELECT cardinality(value)=0 OR illya_valid_map_manifest(value)
$$;
CREATE TABLE local_beatmap_distributions (
    package_id UUID PRIMARY KEY REFERENCES beatmap_packages,
    id UUID NOT NULL UNIQUE,
    native_set_id BIGINT NOT NULL CHECK(native_set_id BETWEEN 2000000000 AND 2147483647),
    format_version SMALLINT NOT NULL CHECK(format_version=1),
    kind TEXT NOT NULL CHECK(kind IN ('osu','osz')),
    original_sha256 TEXT NOT NULL CHECK(original_sha256 ~ '^[0-9a-f]{64}$'),
    original_byte_length BIGINT NOT NULL CHECK(original_byte_length BETWEEN 1 AND 67108864),
    retained_orphan_md5 TEXT[] NOT NULL DEFAULT '{}' CHECK(illya_valid_optional_map_manifest(retained_orphan_md5)),
    archive_path TEXT UNIQUE,
    sha256 TEXT CHECK(sha256 ~ '^[0-9a-f]{64}$'),
    byte_length BIGINT CHECK(byte_length BETWEEN 1 AND 67108864),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK((kind='osu' AND archive_path IS NULL AND sha256 IS NULL AND byte_length IS NULL)
        OR (kind='osz' AND archive_path IS NOT NULL AND archive_path='distribution-'||id::text||'/package.osz'
            AND sha256 IS NOT NULL AND byte_length IS NOT NULL))
);
CREATE TABLE local_beatmap_distribution_members (
    distribution_id UUID NOT NULL REFERENCES local_beatmap_distributions(id),
    beatmap_id BIGINT PRIMARY KEY REFERENCES beatmaps,
    original_md5 TEXT NOT NULL CHECK(original_md5 ~ '^[0-9a-f]{32}$'),
    derived_md5 TEXT NOT NULL UNIQUE CHECK(derived_md5 ~ '^[0-9a-f]{32}$'),
    native_id BIGINT NOT NULL CHECK(native_id BETWEEN 2000000000 AND 2147483647),
    native_set_id BIGINT NOT NULL CHECK(native_set_id BETWEEN 2000000000 AND 2147483647),
    osu_path TEXT NOT NULL UNIQUE,
    sha256 TEXT NOT NULL CHECK(sha256 ~ '^[0-9a-f]{64}$'),
    byte_length BIGINT NOT NULL CHECK(byte_length BETWEEN 1 AND 8388608),
    CHECK(osu_path='distribution-'||distribution_id::text||'/'||derived_md5||'.osu')
);

CREATE FUNCTION illya_preserve_local_distribution() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='local beatmap distributions are immutable';
END $$;
CREATE TRIGGER local_distributions_immutable BEFORE UPDATE OR DELETE ON local_beatmap_distributions
    FOR EACH ROW EXECUTE FUNCTION illya_preserve_local_distribution();
CREATE TRIGGER local_distribution_members_immutable BEFORE UPDATE OR DELETE ON local_beatmap_distribution_members
    FOR EACH ROW EXECUTE FUNCTION illya_preserve_local_distribution();

CREATE FUNCTION illya_guard_local_distribution() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE package beatmap_packages%ROWTYPE;
BEGIN
    SELECT * INTO package FROM beatmap_packages WHERE id=NEW.package_id;
    IF NOT FOUND OR NEW.native_set_id IS DISTINCT FROM package.native_set_id
        OR NEW.kind IS DISTINCT FROM package.kind
        OR (NOT package.legacy AND (NEW.original_sha256 IS DISTINCT FROM package.sha256
            OR NEW.original_byte_length IS DISTINCT FROM package.byte_length)) THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='derived distribution does not match original package';
    END IF;
    PERFORM illya_lock_active_beatmapset(NEW.native_set_id);
    RETURN NEW;
END $$;
CREATE TRIGGER local_distribution_identity BEFORE INSERT ON local_beatmap_distributions
    FOR EACH ROW EXECUTE FUNCTION illya_guard_local_distribution();

-- Both namespaces share the upload checksum fence, including original content
-- inserted after an alias. Lock keys can collide harmlessly; service publishers
-- prelock their complete distinct key list in ascending key order.
CREATE FUNCTION illya_guard_original_checksum() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended('illyaverse:local-map:'||NEW.md5,0));
    IF EXISTS(SELECT 1 FROM local_beatmap_distribution_members m
        WHERE m.derived_md5=NEW.md5 AND m.beatmap_id<>NEW.id) THEN
        RAISE EXCEPTION USING ERRCODE='23505',MESSAGE='original checksum collides with a different derived difficulty';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER beatmaps_z_checksum_identity BEFORE INSERT ON beatmaps
    FOR EACH ROW EXECUTE FUNCTION illya_guard_original_checksum();

CREATE FUNCTION illya_guard_distribution_member() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE map beatmaps%ROWTYPE; distribution local_beatmap_distributions%ROWTYPE;
BEGIN
    SELECT * INTO distribution FROM local_beatmap_distributions WHERE id=NEW.distribution_id;
    IF distribution.id IS NULL THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='derived distribution does not exist';
    END IF;
    PERFORM illya_lock_active_beatmapset(distribution.native_set_id);
    PERFORM pg_advisory_xact_lock(hashtextextended('illyaverse:local-map:'||NEW.derived_md5,0));
    -- Source/ruleset edits must wait until the derivative commits, after which
    -- the distributed difficulty identity trigger rejects those edits.
    SELECT * INTO map FROM beatmaps WHERE id=NEW.beatmap_id FOR SHARE;
    IF distribution.id IS NULL OR map.id IS NULL OR map.source<>'local'
        OR map.official_id IS NOT NULL OR map.local_package_id IS DISTINCT FROM distribution.package_id
        OR NEW.original_md5 IS DISTINCT FROM map.md5 OR NEW.native_id IS DISTINCT FROM map.native_id
        OR NEW.native_set_id IS DISTINCT FROM map.native_set_id
        OR NEW.native_set_id IS DISTINCT FROM distribution.native_set_id THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='derived checksum does not identify its canonical local difficulty';
    END IF;
    IF EXISTS(SELECT 1 FROM beatmaps b WHERE b.md5=NEW.derived_md5 AND b.id<>NEW.beatmap_id) THEN
        RAISE EXCEPTION USING ERRCODE='23505',MESSAGE='derived checksum collides with a different original difficulty';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER local_distribution_member_identity BEFORE INSERT ON local_beatmap_distribution_members
    FOR EACH ROW EXECUTE FUNCTION illya_guard_distribution_member();

CREATE FUNCTION illya_validate_local_distribution() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE distribution local_beatmap_distributions%ROWTYPE; actual TEXT[]; expected TEXT[]; lock_key BIGINT;
BEGIN
    IF TG_TABLE_NAME='local_beatmap_distributions' THEN
        SELECT * INTO distribution FROM local_beatmap_distributions WHERE id=NEW.id;
    ELSE
        SELECT * INTO distribution FROM local_beatmap_distributions WHERE id=NEW.distribution_id;
    END IF;
    SELECT member_md5 INTO expected FROM beatmap_packages WHERE id=distribution.package_id;
    -- Defend direct database publishers as well as the application path. Take
    -- the complete immutable original/derived namespace in actual key order,
    -- then observe any original difficulty committed while this waited.
    FOR lock_key IN
        SELECT DISTINCT hashtextextended('illyaverse:local-map:'||checksum,0) AS key
        FROM (SELECT unnest(expected) AS checksum UNION ALL
            SELECT derived_md5 FROM local_beatmap_distribution_members WHERE distribution_id=distribution.id) checksums
        ORDER BY key
    LOOP
        PERFORM pg_advisory_xact_lock(lock_key);
    END LOOP;
    SELECT array_agg(checksum ORDER BY checksum) INTO actual FROM (
        SELECT original_md5 AS checksum FROM local_beatmap_distribution_members WHERE distribution_id=distribution.id
        UNION ALL SELECT unnest(distribution.retained_orphan_md5)
    ) checksums;
    IF actual IS DISTINCT FROM expected OR EXISTS(
        SELECT 1 FROM beatmaps b WHERE b.local_package_id=distribution.package_id
        AND NOT EXISTS(SELECT 1 FROM local_beatmap_distribution_members m
            WHERE m.distribution_id=distribution.id AND m.beatmap_id=b.id)) OR EXISTS(
        SELECT 1 FROM beatmaps b WHERE b.local_package_id=distribution.package_id
            AND b.md5=ANY(distribution.retained_orphan_md5)) THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='derived distribution requires the complete original package';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER local_distribution_complete AFTER INSERT ON local_beatmap_distributions
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION illya_validate_local_distribution();
CREATE CONSTRAINT TRIGGER local_distribution_member_complete AFTER INSERT ON local_beatmap_distribution_members
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION illya_validate_local_distribution();

-- A chart deleted before the first materialization has no current native ID.
-- Its original bytes stay in the full archive but cannot later be claimed by a
-- new row under an already published package identity.
CREATE FUNCTION illya_guard_published_local_distribution() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- The materializer locks every original manifest checksum, including
    -- deleted charts, before it publishes. Recheck after this wait so a new
    -- insert cannot race an orphan into an already frozen distribution.
    PERFORM pg_advisory_xact_lock(hashtextextended('illyaverse:local-map:'||NEW.md5,0));
    IF NEW.local_package_id IS NOT NULL AND EXISTS(
        SELECT 1 FROM local_beatmap_distributions d WHERE d.package_id=NEW.local_package_id
    ) THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='published local distribution membership is immutable';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER beatmaps_y_published_local_distribution BEFORE INSERT ON beatmaps
    FOR EACH ROW EXECUTE FUNCTION illya_guard_published_local_distribution();

CREATE FUNCTION illya_preserve_distributed_difficulty() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.source IS DISTINCT FROM OLD.source OR NEW.mode IS DISTINCT FROM OLD.mode)
        AND EXISTS(SELECT 1 FROM local_beatmap_distribution_members WHERE beatmap_id=OLD.id) THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='distributed local difficulty source and ruleset are immutable';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER distributed_local_difficulty_identity BEFORE UPDATE ON beatmaps
    FOR EACH ROW EXECUTE FUNCTION illya_preserve_distributed_difficulty();

-- Commit makes the complete aliases and cursor visible together. Clients that
-- fetched the original checksum before downloading can refresh the preferred
-- canonical distribution without changing any original map revision.
CREATE FUNCTION lazer_record_local_distribution_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO lazer_beatmap_updates(beatmap_set_id) VALUES(NEW.native_set_id);
    RETURN NEW;
END $$;
CREATE TRIGGER local_distribution_availability AFTER INSERT ON local_beatmap_distributions
    FOR EACH ROW EXECUTE FUNCTION lazer_record_local_distribution_update();

CREATE VIEW beatmap_checksum_identities AS
    SELECT b.md5,b.id AS beatmap_id,FALSE AS derived FROM beatmaps b
    UNION ALL
    SELECT m.derived_md5,m.beatmap_id,TRUE FROM local_beatmap_distribution_members m
        JOIN local_beatmap_distributions d ON d.id=m.distribution_id;
CREATE FUNCTION illya_beatmap_checksum_matches(internal_id BIGINT,checksum TEXT) RETURNS BOOLEAN
LANGUAGE sql STABLE STRICT AS $$
    SELECT checksum ~ '^[0-9a-f]{32}$' AND EXISTS(
        SELECT 1 FROM beatmap_checksum_identities i WHERE i.beatmap_id=internal_id AND i.md5=checksum)
$$;
CREATE FUNCTION illya_beatmap_preferred_checksum(internal_id BIGINT) RETURNS TEXT
LANGUAGE sql STABLE STRICT AS $$
    SELECT COALESCE(m.derived_md5,b.md5) FROM beatmaps b
        LEFT JOIN local_beatmap_distribution_members m ON m.beatmap_id=b.id WHERE b.id=internal_id
$$;

-- No backfill: a legacy NULL remains bound to the original checksum. New native
-- admissions explicitly freeze their exact accepted client checksum.
ALTER TABLE lazer_score_tokens ADD COLUMN client_beatmap_md5 TEXT
    CHECK(client_beatmap_md5 ~ '^[0-9a-f]{32}$');
ALTER TABLE scores ADD COLUMN client_beatmap_md5 TEXT
    CHECK(client_beatmap_md5 ~ '^[0-9a-f]{32}$');
CREATE FUNCTION illya_bind_client_beatmap_checksum() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE set_id BIGINT;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF NEW.client_beatmap_md5 IS DISTINCT FROM OLD.client_beatmap_md5
            OR NEW.beatmap_id IS DISTINCT FROM OLD.beatmap_id THEN
            RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='admitted client checksum is immutable';
        END IF;
    ELSIF NEW.client_beatmap_md5 IS NOT NULL THEN
        SELECT native_set_id INTO set_id FROM beatmaps WHERE id=NEW.beatmap_id;
        IF set_id IS NOT NULL THEN
            PERFORM illya_lock_beatmapset(set_id);
        END IF;
        IF NOT illya_beatmap_checksum_matches(NEW.beatmap_id,NEW.client_beatmap_md5) THEN
            RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='client checksum must identify the admitted canonical difficulty';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER lazer_score_tokens_client_checksum BEFORE INSERT OR UPDATE ON lazer_score_tokens
    FOR EACH ROW EXECUTE FUNCTION illya_bind_client_beatmap_checksum();
CREATE TRIGGER scores_client_checksum BEFORE INSERT OR UPDATE ON scores
    FOR EACH ROW EXECUTE FUNCTION illya_bind_client_beatmap_checksum();
