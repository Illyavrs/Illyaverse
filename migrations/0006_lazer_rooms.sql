-- Lazer's multiplayer rooms deliberately remain separate from stable matches.
-- Durable room state is authoritative; frame streams and transport queues are transient.
CREATE TABLE lazer_rooms (
    id BIGSERIAL PRIMARY KEY,
    host_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    channel_id BIGINT REFERENCES channels ON DELETE SET NULL,
    state SMALLINT NOT NULL DEFAULT 0 CHECK(state BETWEEN 0 AND 3),
    settings JSONB NOT NULL,
    playlist JSONB NOT NULL DEFAULT '[]',
    locked BOOLEAN NOT NULL DEFAULT FALSE,
    revision BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE lazer_room_members (
    room_id BIGINT NOT NULL REFERENCES lazer_rooms ON DELETE CASCADE,
    user_id BIGINT NOT NULL UNIQUE REFERENCES users ON DELETE CASCADE,
    state SMALLINT NOT NULL DEFAULT 0 CHECK(state BETWEEN 0 AND 8),
    beatmap_availability JSONB NOT NULL DEFAULT '{"state":0,"downloadProgress":null}',
    mods JSONB NOT NULL DEFAULT '[]',
    beatmap_id BIGINT,
    ruleset_id SMALLINT CHECK(ruleset_id BETWEEN 0 AND 3),
    voted_to_skip BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(room_id,user_id)
);
CREATE INDEX lazer_rooms_open ON lazer_rooms(state,updated_at);
CREATE TABLE lazer_beatmap_updates (
    id SERIAL PRIMARY KEY,
    beatmap_set_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE FUNCTION lazer_record_beatmap_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.official_set_id IS NOT NULL THEN
        INSERT INTO lazer_beatmap_updates(beatmap_set_id) VALUES(NEW.official_set_id);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER lazer_map_change AFTER INSERT OR UPDATE OF status ON beatmaps
FOR EACH ROW EXECUTE FUNCTION lazer_record_beatmap_update();
