ALTER TABLE bancho_sessions ADD COLUMN client_version TEXT NOT NULL DEFAULT '';
ALTER TABLE bancho_sessions ADD COLUMN client_hash_hash TEXT NOT NULL DEFAULT '';
CREATE TABLE bancho_packet_queue (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    payload BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now()+interval '5 minutes'
);
CREATE INDEX bancho_packet_queue_user ON bancho_packet_queue(user_id,id);
