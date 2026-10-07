-- Replay capture is durable across reconnects and score/end ordering races.
-- Frame bytes are discarded once the atomic replay file is published.
ALTER TABLE scores ADD COLUMN replay_sha256 TEXT;
ALTER TABLE scores ADD COLUMN replay_format TEXT CHECK(replay_format IN ('legacy','lazer'));
-- NULL for uploads/older files whose stream continuity was never observed.
ALTER TABLE scores ADD COLUMN replay_complete BOOLEAN;
CREATE TABLE lazer_replay_captures (
    token_id BIGINT PRIMARY KEY REFERENCES lazer_score_tokens ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    connection_key TEXT NOT NULL,
    initial_state JSONB NOT NULL,
    final_header JSONB,
    frame_bytes INTEGER NOT NULL DEFAULT 0 CHECK(frame_bytes BETWEEN 0 AND 33554432),
    frame_count INTEGER NOT NULL DEFAULT 0 CHECK(frame_count BETWEEN 0 AND 1000000),
    last_time INTEGER NOT NULL DEFAULT 0,
    last_bundle_hash TEXT,
    interrupted BOOLEAN NOT NULL DEFAULT false,
    final_state SMALLINT CHECK(final_state BETWEEN 3 AND 5),
    ended_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    abandoned_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX lazer_replay_capture_owner ON lazer_replay_captures(user_id,updated_at);
CREATE TABLE lazer_replay_chunks (
    token_id BIGINT NOT NULL REFERENCES lazer_replay_captures ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK(sequence BETWEEN 0 AND 999999),
    frame_text BYTEA NOT NULL CHECK(octet_length(frame_text) BETWEEN 1 AND 65536),
    PRIMARY KEY(token_id,sequence)
);
