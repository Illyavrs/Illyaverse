-- Validated public metadata is distinct from content, archives and private nominations.
CREATE TABLE beatmap_catalogue_metadata (
    native_set_id BIGINT PRIMARY KEY CHECK (native_set_id BETWEEN 1 AND 1999999999),
    metadata JSONB NOT NULL CHECK (jsonb_typeof(metadata) = 'object'
        AND octet_length(metadata::text) <= 4194304),
    revision BIGINT NOT NULL DEFAULT 0 CHECK (revision >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    verified_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE FUNCTION illya_catalogue_metadata_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.revision := 0;
    ELSE
        IF NEW.native_set_id IS DISTINCT FROM OLD.native_set_id THEN
            RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'catalogue set identity is immutable';
        END IF;
        NEW.revision := OLD.revision + CASE WHEN NEW.metadata IS DISTINCT FROM OLD.metadata THEN 1 ELSE 0 END;
        NEW.created_at := OLD.created_at;
    END IF;
    NEW.verified_at := clock_timestamp();
    RETURN NEW;
END;
$$;
CREATE TRIGGER beatmap_catalogue_metadata_revision BEFORE INSERT OR UPDATE
    ON beatmap_catalogue_metadata FOR EACH ROW EXECUTE FUNCTION illya_catalogue_metadata_revision();

-- A native continuation is invalidated by changes to membership or ordering.
-- No score/activity write touches this generation.
CREATE TABLE server_catalogue_revision (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    epoch BIGINT NOT NULL DEFAULT 0 CHECK (epoch >= 0)
);
INSERT INTO server_catalogue_revision(singleton) VALUES (TRUE);
CREATE FUNCTION illya_advance_server_catalogue() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE server_catalogue_revision SET epoch = epoch + 1 WHERE singleton;
    RETURN NULL;
END;
$$;
CREATE TRIGGER beatmaps_server_catalogue_revision AFTER INSERT OR UPDATE OR DELETE
    ON beatmaps FOR EACH STATEMENT EXECUTE FUNCTION illya_advance_server_catalogue();
CREATE TRIGGER beatmap_overrides_server_catalogue_revision AFTER INSERT OR UPDATE OR DELETE
    ON beatmap_status_overrides FOR EACH STATEMENT EXECUTE FUNCTION illya_advance_server_catalogue();
CREATE TRIGGER beatmap_availability_server_catalogue_revision AFTER INSERT OR UPDATE OR DELETE
    ON beatmapset_availability FOR EACH STATEMENT EXECUTE FUNCTION illya_advance_server_catalogue();
CREATE TRIGGER beatmap_metadata_server_catalogue_revision AFTER INSERT OR UPDATE OR DELETE
    ON beatmap_catalogue_metadata FOR EACH STATEMENT EXECUTE FUNCTION illya_advance_server_catalogue();
CREATE TRIGGER beatmap_packages_server_catalogue_revision AFTER INSERT OR UPDATE OR DELETE
    ON beatmap_packages FOR EACH STATEMENT EXECUTE FUNCTION illya_advance_server_catalogue();
CREATE TRIGGER official_archives_server_catalogue_revision AFTER INSERT OR UPDATE OR DELETE
    ON official_beatmapset_archives FOR EACH STATEMENT EXECUTE FUNCTION illya_advance_server_catalogue();
CREATE TRIGGER official_archive_members_server_catalogue_revision AFTER INSERT OR UPDATE OR DELETE
    ON official_beatmapset_archive_members FOR EACH STATEMENT EXECUTE FUNCTION illya_advance_server_catalogue();
