-- Operator-provided, fully verified official set archives. Difficulty identity,
-- ownership and score/replay foreign keys remain immutable.
CREATE TABLE official_beatmapset_archives (
    official_set_id BIGINT PRIMARY KEY CHECK (official_set_id BETWEEN 1 AND 1999999999),
    id UUID NOT NULL UNIQUE DEFAULT gen_random_uuid(),
    archive_path TEXT NOT NULL UNIQUE CHECK (archive_path ~ '^official-package-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/package\.osz$'),
    sha256 TEXT NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    byte_length BIGINT NOT NULL CHECK (byte_length BETWEEN 1 AND 67108864),
    member_md5 TEXT[] NOT NULL CHECK (illya_valid_map_manifest(member_md5)),
    uploaded_by BIGINT NOT NULL REFERENCES users,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE official_beatmapset_archive_members (
    archive_id UUID NOT NULL REFERENCES official_beatmapset_archives(id),
    beatmap_id BIGINT NOT NULL UNIQUE REFERENCES beatmaps(id),
    PRIMARY KEY (archive_id,beatmap_id)
);

-- Availability changes even when every difficulty was already cached. Publish
-- the existing native update cursor event atomically with the complete archive.
CREATE FUNCTION lazer_record_official_archive_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO lazer_beatmap_updates(beatmap_set_id) VALUES(NEW.official_set_id);
    RETURN NEW;
END $$;
CREATE TRIGGER official_archive_availability AFTER INSERT ON official_beatmapset_archives
    FOR EACH ROW EXECUTE FUNCTION lazer_record_official_archive_update();

CREATE FUNCTION illya_preserve_official_archive() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='official archive content and membership are immutable';
END $$;
CREATE TRIGGER official_archives_immutable BEFORE UPDATE OR DELETE ON official_beatmapset_archives
    FOR EACH ROW EXECUTE FUNCTION illya_preserve_official_archive();
CREATE TRIGGER official_archive_members_immutable BEFORE UPDATE OR DELETE ON official_beatmapset_archive_members
    FOR EACH ROW EXECUTE FUNCTION illya_preserve_official_archive();

CREATE FUNCTION illya_preserve_archived_difficulty() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.source IS DISTINCT FROM OLD.source OR NEW.mode IS DISTINCT FROM OLD.mode)
        AND EXISTS(SELECT 1 FROM official_beatmapset_archive_members WHERE beatmap_id=OLD.id) THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='archived difficulty source and ruleset are immutable';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER archived_difficulty_identity BEFORE UPDATE ON beatmaps
    FOR EACH ROW EXECUTE FUNCTION illya_preserve_archived_difficulty();

CREATE FUNCTION illya_validate_official_archive() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    archive official_beatmapset_archives%ROWTYPE;
    content_id UUID;
    actual TEXT[];
BEGIN
    IF TG_TABLE_NAME='official_beatmapset_archives' THEN
        content_id := NEW.id;
    ELSE
        content_id := NEW.archive_id;
    END IF;
    SELECT * INTO archive FROM official_beatmapset_archives WHERE id=content_id;
    SELECT array_agg(b.md5 ORDER BY b.md5) INTO actual
        FROM official_beatmapset_archive_members m JOIN beatmaps b ON b.id=m.beatmap_id
        WHERE m.archive_id=content_id;
    IF actual IS DISTINCT FROM archive.member_md5 OR EXISTS(
        SELECT 1 FROM official_beatmapset_archive_members m JOIN beatmaps b ON b.id=m.beatmap_id
        WHERE m.archive_id=content_id AND
            (b.source <> 'official' OR b.official_id IS NULL OR b.official_set_id IS DISTINCT FROM archive.official_set_id)
    ) OR EXISTS(
        SELECT 1 FROM beatmaps b WHERE b.native_set_id=archive.official_set_id AND b.official_set_id=archive.official_set_id
        AND NOT EXISTS(SELECT 1 FROM official_beatmapset_archive_members m
            WHERE m.archive_id=content_id AND m.beatmap_id=b.id)
    ) THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='official archive does not match its complete immutable set';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER official_archive_complete AFTER INSERT ON official_beatmapset_archives
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION illya_validate_official_archive();
CREATE CONSTRAINT TRIGGER official_archive_member_complete AFTER INSERT ON official_beatmapset_archive_members
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION illya_validate_official_archive();

-- A later single-difficulty publication cannot extend a downloadable archive.
CREATE FUNCTION illya_guard_published_official_set() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM official_beatmapset_archives a WHERE a.official_set_id=NEW.official_set_id)
        AND NOT EXISTS(SELECT 1 FROM official_beatmapset_archives a
            JOIN official_beatmapset_archive_members m ON m.archive_id=a.id
            WHERE a.official_set_id=NEW.official_set_id AND m.beatmap_id=NEW.id) THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='difficulty is absent from its published official archive';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER published_official_set_members AFTER INSERT ON beatmaps
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION illya_guard_published_official_set();
