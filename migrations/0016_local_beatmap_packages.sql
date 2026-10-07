-- Immutable local upload packages and bounded native client identities.
-- Official IDs remain unchanged below the service's managed local range.
-- Internal beatmap/score/token keys and retained content bytes do not change.
DO $$
BEGIN
    IF EXISTS(SELECT 1 FROM lazer_score_tokens t JOIN beatmaps b ON b.id=t.beatmap_id
        WHERE t.scope='room' AND t.processing_status='received' AND b.official_id IS NULL) THEN
        RAISE EXCEPTION USING ERRCODE='55000',
            MESSAGE='Drain received local room scores before upgrading native map IDs';
    END IF;
END $$;

CREATE SEQUENCE local_beatmap_native_ids AS BIGINT
    START WITH 2000000000 MINVALUE 2000000000 MAXVALUE 2147483647 NO CYCLE;
CREATE SEQUENCE local_beatmapset_native_ids AS BIGINT
    START WITH 2000000000 MINVALUE 2000000000 MAXVALUE 2147483647 NO CYCLE;

CREATE FUNCTION illya_valid_map_manifest(value TEXT[]) RETURNS BOOLEAN
    LANGUAGE sql IMMUTABLE STRICT AS $$
    SELECT cardinality(value) BETWEEN 1 AND 512
        AND array_position(value,NULL) IS NULL
        AND value = ARRAY(SELECT DISTINCT entry FROM unnest(value) entry ORDER BY entry)
        AND (SELECT bool_and(entry ~ '^[0-9a-f]{32}$') FROM unnest(value) entry)
$$;

CREATE TABLE beatmap_packages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    native_set_id BIGINT NOT NULL UNIQUE DEFAULT nextval('local_beatmapset_native_ids')
        CHECK (native_set_id BETWEEN 2000000000 AND 2147483647),
    uploaded_by BIGINT REFERENCES users,
    kind TEXT NOT NULL CHECK (kind IN ('osu','osz')),
    sha256 TEXT CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    byte_length BIGINT CHECK (byte_length BETWEEN 1 AND 67108864),
    member_md5 TEXT[] NOT NULL CHECK (illya_valid_map_manifest(member_md5)),
    archive_path TEXT,
    legacy BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((sha256 IS NULL) = (byte_length IS NULL)),
    CHECK (legacy = (sha256 IS NULL)),
    CHECK ((kind='osu' AND archive_path IS NULL AND cardinality(member_md5)=1)
        OR (kind='osz' AND archive_path IS NOT NULL)),
    UNIQUE (uploaded_by,kind,sha256)
);

ALTER TABLE beatmaps
    ADD COLUMN local_package_id UUID REFERENCES beatmap_packages,
    ADD COLUMN local_native_id BIGINT CHECK (local_native_id BETWEEN 2000000000 AND 2147483647),
    ADD COLUMN native_set_id BIGINT,
    ADD CONSTRAINT beatmaps_official_native_range CHECK
        (official_id IS NULL OR official_id BETWEEN 1 AND 1999999999),
    ADD CONSTRAINT beatmaps_official_set_native_range CHECK
        (official_set_id IS NULL OR official_set_id BETWEEN 1 AND 1999999999);

-- Group only the boundary created by the previous local upload implementation:
-- one UUID directory, one package.osz, matching per-MD5 paths, and one owner.
-- No title/artist/creator or embedded official metadata is used as identity.
-- Retained files are NOT read by a migration, so legacy fingerprints stay NULL.
DO $$
DECLARE
    grouping RECORD;
    singleton RECORD;
    package_id UUID;
    public_set_id BIGINT;
BEGIN
    FOR grouping IN
        SELECT archive_path, min(uploaded_by) owner, min(created_at) created,
            array_agg(md5 ORDER BY md5) manifest
        FROM beatmaps
        WHERE archive_path IS NOT NULL
        GROUP BY archive_path
        HAVING count(DISTINCT uploaded_by) = CASE WHEN bool_or(uploaded_by IS NULL) THEN 0 ELSE 1 END
            AND count(*) <= 512
            AND bool_and(source='local' AND official_id IS NULL AND official_set_id IS NULL
                AND archive_path ~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/package\.osz$'
                AND osu_path = regexp_replace(archive_path,'package\.osz$',md5||'.osu'))
        ORDER BY archive_path
    LOOP
        INSERT INTO beatmap_packages(uploaded_by,kind,member_md5,archive_path,legacy,created_at)
            VALUES(grouping.owner,'osz',grouping.manifest,grouping.archive_path,TRUE,grouping.created)
            RETURNING id,native_set_id INTO package_id,public_set_id;
        UPDATE beatmaps SET local_package_id=package_id,native_set_id=public_set_id
            WHERE official_set_id IS NULL AND archive_path=grouping.archive_path;
    END LOOP;

    FOR singleton IN
        SELECT * FROM beatmaps WHERE official_set_id IS NULL AND local_package_id IS NULL ORDER BY id
    LOOP
        INSERT INTO beatmap_packages(uploaded_by,kind,member_md5,archive_path,legacy,created_at)
            VALUES(singleton.uploaded_by,CASE WHEN singleton.archive_path IS NULL THEN 'osu' ELSE 'osz' END,
                ARRAY[singleton.md5],singleton.archive_path,TRUE,singleton.created_at)
            RETURNING id,native_set_id INTO package_id,public_set_id;
        UPDATE beatmaps SET local_package_id=package_id,native_set_id=public_set_id WHERE id=singleton.id;
    END LOOP;

    FOR singleton IN SELECT id FROM beatmaps WHERE official_id IS NULL ORDER BY id LOOP
        UPDATE beatmaps SET local_native_id=nextval('local_beatmap_native_ids') WHERE id=singleton.id;
    END LOOP;
END $$;
UPDATE beatmaps SET native_set_id=official_set_id WHERE official_set_id IS NOT NULL;
ALTER TABLE beatmaps
    ADD COLUMN native_id BIGINT GENERATED ALWAYS AS (COALESCE(official_id,local_native_id)) STORED,
    ALTER COLUMN native_id SET NOT NULL,
    ALTER COLUMN native_set_id SET NOT NULL,
    ADD CONSTRAINT beatmaps_native_id_unique UNIQUE(native_id),
    ADD CONSTRAINT beatmaps_native_set_range CHECK(native_set_id BETWEEN 1 AND 2147483647),
    ADD CONSTRAINT beatmaps_native_identity_shape CHECK
        ((official_id IS NULL) = (local_native_id IS NOT NULL)),
    ADD CONSTRAINT beatmaps_native_set_shape CHECK
        ((official_set_id IS NULL) = (local_package_id IS NOT NULL));
CREATE INDEX beatmaps_native_set_members ON beatmaps(native_set_id,id);
CREATE INDEX beatmaps_local_package_members ON beatmaps(local_package_id,id);

CREATE FUNCTION illya_bind_native_map_identity() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    package beatmap_packages%ROWTYPE;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF NEW.id IS DISTINCT FROM OLD.id OR NEW.md5 IS DISTINCT FROM OLD.md5
            OR NEW.official_id IS DISTINCT FROM OLD.official_id
            OR NEW.official_set_id IS DISTINCT FROM OLD.official_set_id
            OR NEW.local_native_id IS DISTINCT FROM OLD.local_native_id
            OR NEW.local_package_id IS DISTINCT FROM OLD.local_package_id
            OR NEW.native_set_id IS DISTINCT FROM OLD.native_set_id
            OR NEW.uploaded_by IS DISTINCT FROM OLD.uploaded_by
            OR NEW.osu_path IS DISTINCT FROM OLD.osu_path
            OR (NEW.archive_path IS DISTINCT FROM OLD.archive_path
                AND NOT (OLD.official_set_id IS NOT NULL AND OLD.archive_path IS NULL
                    AND NEW.archive_path IS NOT NULL)) THEN
            RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='beatmap content and native identity are immutable';
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.official_id IS NULL AND NEW.local_native_id IS NULL THEN
        NEW.local_native_id := nextval('local_beatmap_native_ids');
    END IF;
    IF NEW.official_set_id IS NOT NULL THEN
        IF NEW.local_package_id IS NOT NULL THEN
            RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='official sets cannot claim a local package';
        END IF;
        NEW.native_set_id := NEW.official_set_id;
    ELSE
        -- Also covers standalone legacy/import fixtures. Uploads supply a
        -- complete, fingerprinted package before inserting their members.
        IF NEW.local_package_id IS NULL THEN
            INSERT INTO beatmap_packages(uploaded_by,kind,member_md5,archive_path,legacy)
                VALUES(NEW.uploaded_by,CASE WHEN NEW.archive_path IS NULL THEN 'osu' ELSE 'osz' END,
                    ARRAY[NEW.md5],NEW.archive_path,TRUE)
                RETURNING id INTO NEW.local_package_id;
        END IF;
        SELECT * INTO package FROM beatmap_packages WHERE id=NEW.local_package_id FOR KEY SHARE;
        IF NOT FOUND OR package.uploaded_by IS DISTINCT FROM NEW.uploaded_by
            OR package.archive_path IS DISTINCT FROM NEW.archive_path
            OR NOT (NEW.md5 = ANY(package.member_md5)) THEN
            RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='beatmap does not match its immutable package';
        END IF;
        NEW.native_set_id := package.native_set_id;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER beatmaps_bind_native_identity BEFORE INSERT OR UPDATE ON beatmaps
    FOR EACH ROW EXECUTE FUNCTION illya_bind_native_map_identity();

CREATE FUNCTION illya_preserve_map_package() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id OR NEW.native_set_id IS DISTINCT FROM OLD.native_set_id
        OR NEW.uploaded_by IS DISTINCT FROM OLD.uploaded_by OR NEW.kind IS DISTINCT FROM OLD.kind
        OR NEW.member_md5 IS DISTINCT FROM OLD.member_md5
        OR NEW.archive_path IS DISTINCT FROM OLD.archive_path
        OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='beatmap package membership and identity are immutable';
    END IF;
    IF NOT OLD.legacy AND (NEW.sha256 IS DISTINCT FROM OLD.sha256
        OR NEW.byte_length IS DISTINCT FROM OLD.byte_length OR NEW.legacy IS DISTINCT FROM OLD.legacy) THEN
        RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='beatmap package fingerprint is immutable';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER beatmap_packages_immutable BEFORE UPDATE ON beatmap_packages
    FOR EACH ROW EXECUTE FUNCTION illya_preserve_map_package();

CREATE OR REPLACE FUNCTION lazer_record_beatmap_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO lazer_beatmap_updates(beatmap_set_id) VALUES(NEW.native_set_id);
    RETURN NEW;
END $$;
INSERT INTO lazer_beatmap_updates(beatmap_set_id) SELECT native_set_id FROM beatmap_packages;

-- Persisted playlists carry public IDs, unlike tokens/rounds/scores whose
-- foreign keys already bind immutable internal rows. A checksum identifies
-- each playlist's exact row even when its old small local ID collided.
CREATE FUNCTION illya_rebind_playlist_native_ids(value JSONB) RETURNS JSONB LANGUAGE sql STABLE AS $$
    SELECT COALESCE(jsonb_agg(CASE WHEN b.id IS NULL THEN item ELSE
        jsonb_set(item,'{beatmapID}',to_jsonb(b.native_id)) END ORDER BY ordinal),'[]'::jsonb)
    FROM jsonb_array_elements(value) WITH ORDINALITY AS items(item,ordinal)
    LEFT JOIN beatmaps b ON b.md5=item->>'beatmapChecksum'
$$;
UPDATE lazer_rooms SET playlist=illya_rebind_playlist_native_ids(playlist),
    revision=revision+1,updated_at=now()
    WHERE playlist IS DISTINCT FROM illya_rebind_playlist_native_ids(playlist);
UPDATE lazer_room_playlist_versions SET snapshot=jsonb_set(snapshot,'{beatmapID}',to_jsonb(b.native_id))
    FROM beatmaps b WHERE b.md5=snapshot->>'beatmapChecksum'
        AND snapshot->'beatmapID' IS DISTINCT FROM to_jsonb(b.native_id);
UPDATE lazer_room_members m SET beatmap_id=b.native_id FROM beatmaps b
    WHERE b.official_id IS NULL AND m.beatmap_id=b.id
        AND NOT EXISTS(SELECT 1 FROM beatmaps official WHERE official.official_id=m.beatmap_id);
-- Capture state is bound by its token FK, rather than an ambiguous old public
-- ID. Rebind only that public field, keeping every original chunk/hash/header.
UPDATE lazer_replay_captures c SET initial_state=jsonb_set(c.initial_state,'{beatmapID}',to_jsonb(b.native_id)),updated_at=now()
    FROM lazer_score_tokens t JOIN beatmaps b ON b.id=t.beatmap_id
    WHERE c.token_id=t.id AND b.official_id IS NULL
        AND jsonb_typeof(c.initial_state)='object'
        AND c.initial_state->'beatmapID' IS DISTINCT FROM to_jsonb(b.native_id);
DROP FUNCTION illya_rebind_playlist_native_ids(JSONB);
