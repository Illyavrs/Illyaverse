-- pg_restore imports data with an empty search_path. Resolve the nested
-- validator explicitly so restoring derived beatmap packages remains safe.
-- Keep existing migration checksums, constraints, and package data intact.
CREATE OR REPLACE FUNCTION public.illya_valid_optional_map_manifest(value TEXT[])
RETURNS BOOLEAN LANGUAGE sql IMMUTABLE STRICT AS $$
    SELECT cardinality(value) = 0 OR public.illya_valid_map_manifest(value)
$$;
