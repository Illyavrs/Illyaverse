CREATE TABLE stable_rooms (
    id SMALLINT PRIMARY KEY CHECK (id > 0),
    host_id BIGINT NOT NULL REFERENCES users,
    password_hash TEXT,
    state JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE stable_room_members (
    user_id BIGINT PRIMARY KEY REFERENCES users ON DELETE CASCADE,
    room_id SMALLINT NOT NULL REFERENCES stable_rooms ON DELETE CASCADE,
    slot SMALLINT NOT NULL CHECK (slot BETWEEN 0 AND 15),
    loaded BOOLEAN NOT NULL DEFAULT false,
    skipped BOOLEAN NOT NULL DEFAULT false,
    failed BOOLEAN NOT NULL DEFAULT false,
    UNIQUE(room_id,slot)
);
ALTER TABLE bancho_sessions ADD COLUMN lobby BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE bancho_sessions ADD COLUMN spectator_host_id BIGINT REFERENCES users ON DELETE SET NULL;
CREATE INDEX bancho_spectator_host ON bancho_sessions(spectator_host_id) WHERE spectator_host_id IS NOT NULL;
