CREATE TABLE bancho_sessions (
    id UUID PRIMARY KEY,
    token_hash TEXT UNIQUE NOT NULL,
    user_id BIGINT NOT NULL UNIQUE REFERENCES users ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    last_chat_id BIGINT NOT NULL DEFAULT 0,
    last_dm_id BIGINT NOT NULL DEFAULT 0,
    channels BIGINT[] NOT NULL DEFAULT '{}',
    action JSONB NOT NULL DEFAULT '{}',
    utc_offset SMALLINT NOT NULL DEFAULT 0,
    block_non_friend_dms BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX bancho_sessions_expiry ON bancho_sessions(expires_at);
CREATE TABLE protocol_private_messages (
    id BIGSERIAL PRIMARY KEY,
    sender_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    recipient_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    content TEXT NOT NULL CHECK(length(content) BETWEEN 1 AND 1000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX protocol_private_recipient ON protocol_private_messages(recipient_id,id);
CREATE TABLE lazer_sessions (
    id UUID PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    access_hash TEXT UNIQUE NOT NULL,
    refresh_hash TEXT UNIQUE NOT NULL,
    access_expires_at TIMESTAMPTZ NOT NULL,
    refresh_expires_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX lazer_sessions_expiry ON lazer_sessions(refresh_expires_at);
CREATE TABLE lazer_score_tokens (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    beatmap_id BIGINT NOT NULL REFERENCES beatmaps ON DELETE CASCADE,
    ruleset_id SMALLINT NOT NULL CHECK(ruleset_id BETWEEN 0 AND 3),
    version_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    score_id BIGINT REFERENCES scores,
    submission JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE scores ADD COLUMN ingestion_key TEXT UNIQUE;
CREATE TABLE lazer_channel_members (
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    channel_id BIGINT NOT NULL REFERENCES channels ON DELETE CASCADE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(user_id,channel_id)
);
