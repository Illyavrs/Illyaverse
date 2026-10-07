-- Exact, retained native filename aliases. Existing content and identities are
-- unchanged; legacy names require separate verification rather than guessing.
CREATE TABLE beatmap_filename_aliases (
    filename TEXT COLLATE "C" NOT NULL CHECK (octet_length(filename) BETWEEN 5 AND 1024),
    beatmap_id BIGINT NOT NULL REFERENCES beatmaps(id) ON DELETE CASCADE,
    PRIMARY KEY (filename,beatmap_id)
);
CREATE INDEX beatmap_filename_aliases_map ON beatmap_filename_aliases(beatmap_id);
