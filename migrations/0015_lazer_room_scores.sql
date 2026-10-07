-- Original durable bindings for native room submissions. Solo/replay token IDs
-- retain their shared sequence; historical solo rows are never reclassified.
ALTER TABLE lazer_rooms ADD COLUMN next_playlist_item_id BIGINT NOT NULL DEFAULT 1
    CHECK (next_playlist_item_id > 0);
UPDATE lazer_rooms r SET next_playlist_item_id=COALESCE(
    (SELECT max((item->>'id')::bigint)+1 FROM jsonb_array_elements(r.playlist) item),1);

CREATE TABLE lazer_room_playlist_versions (
    id BIGSERIAL PRIMARY KEY,
    room_id BIGINT NOT NULL REFERENCES lazer_rooms ON DELETE CASCADE,
    playlist_item_id BIGINT NOT NULL CHECK (playlist_item_id > 0),
    snapshot JSONB NOT NULL CHECK (jsonb_typeof(snapshot)='object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id,room_id,playlist_item_id)
);
CREATE INDEX lazer_playlist_version_lookup
    ON lazer_room_playlist_versions(room_id,playlist_item_id,id DESC);
INSERT INTO lazer_room_playlist_versions(room_id,playlist_item_id,snapshot)
    SELECT r.id,(item->>'id')::bigint,item
    FROM lazer_rooms r CROSS JOIN LATERAL jsonb_array_elements(r.playlist) item;

CREATE TABLE lazer_room_rounds (
    id BIGSERIAL PRIMARY KEY,
    room_id BIGINT NOT NULL REFERENCES lazer_rooms ON DELETE CASCADE,
    playlist_item_id BIGINT NOT NULL CHECK (playlist_item_id > 0),
    playlist_version_id BIGINT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('loading','playing','finished','aborted')),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now()+interval '6 hours',
    UNIQUE (id,room_id,playlist_item_id),
    FOREIGN KEY (playlist_version_id,room_id,playlist_item_id)
        REFERENCES lazer_room_playlist_versions(id,room_id,playlist_item_id)
);
CREATE INDEX lazer_room_round_lookup ON lazer_room_rounds(room_id,playlist_item_id,id DESC);
ALTER TABLE lazer_rooms ADD COLUMN current_round_id BIGINT REFERENCES lazer_room_rounds;

CREATE TABLE lazer_room_round_participants (
    round_id BIGINT NOT NULL REFERENCES lazer_room_rounds ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    beatmap_id BIGINT NOT NULL REFERENCES beatmaps,
    beatmap_checksum TEXT NOT NULL CHECK (beatmap_checksum ~ '^[0-9a-f]{32}$'),
    ruleset_id SMALLINT NOT NULL CHECK (ruleset_id BETWEEN 0 AND 3),
    mods JSONB NOT NULL CHECK (jsonb_typeof(mods)='array'),
    aborted_at TIMESTAMPTZ,
    PRIMARY KEY (round_id,user_id)
);
CREATE INDEX lazer_round_participant_history ON lazer_room_round_participants(user_id,round_id);

ALTER TABLE lazer_score_tokens
    ADD COLUMN scope TEXT NOT NULL DEFAULT 'solo' CHECK (scope IN ('solo','room')),
    ADD COLUMN room_id BIGINT,
    ADD COLUMN playlist_item_id BIGINT,
    ADD COLUMN round_id BIGINT,
    ADD COLUMN received_at TIMESTAMPTZ,
    ADD COLUMN received_session_id UUID,
    ADD COLUMN submission_sha256 TEXT CHECK (submission_sha256 ~ '^[0-9a-f]{64}$'),
    ADD COLUMN revoked_at TIMESTAMPTZ,
    ADD COLUMN revoke_reason TEXT,
    ADD COLUMN processing_status TEXT NOT NULL DEFAULT 'issued'
        CHECK (processing_status IN ('issued','received','processed','revoked')),
    ADD COLUMN retry_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN processing_attempts INTEGER NOT NULL DEFAULT 0 CHECK (processing_attempts >= 0),
    ADD COLUMN processing_error TEXT,
    ADD CONSTRAINT lazer_token_scope_binding CHECK (
        (scope='solo' AND room_id IS NULL AND playlist_item_id IS NULL AND round_id IS NULL)
        OR (scope='room' AND room_id IS NOT NULL AND playlist_item_id IS NOT NULL AND round_id IS NOT NULL)
    ),
    ADD CONSTRAINT lazer_token_round_identity FOREIGN KEY (round_id,room_id,playlist_item_id)
        REFERENCES lazer_room_rounds(id,room_id,playlist_item_id),
    ADD CONSTRAINT lazer_token_participant FOREIGN KEY (round_id,user_id)
        REFERENCES lazer_room_round_participants(round_id,user_id),
    ADD CONSTRAINT lazer_room_received_payload CHECK (
        scope<>'room' OR processing_status NOT IN ('received','processed')
        OR (submission IS NOT NULL AND submission_sha256 IS NOT NULL AND received_at IS NOT NULL)
    ),
    ADD CONSTRAINT lazer_room_processed_link CHECK (
        scope<>'room' OR processing_status<>'processed'
        OR (score_id IS NOT NULL AND consumed_at IS NOT NULL)
    );
-- No FK on received_session_id: deleting credentials must never be blocked by
-- retained receipts. Recovery explicitly verifies that the session still exists.
CREATE UNIQUE INDEX lazer_room_one_attempt_per_round
    ON lazer_score_tokens(round_id,user_id) WHERE scope='room';
CREATE INDEX lazer_room_pending_processing
    ON lazer_score_tokens(retry_at,id) WHERE scope='room' AND processing_status='received';
CREATE INDEX lazer_room_score_lookup
    ON lazer_score_tokens(room_id,playlist_item_id,score_id) WHERE scope='room';

CREATE TABLE lazer_room_score_events (
    id BIGSERIAL PRIMARY KEY,
    token_id BIGINT NOT NULL UNIQUE REFERENCES lazer_score_tokens ON DELETE CASCADE,
    room_id BIGINT NOT NULL REFERENCES lazer_rooms ON DELETE CASCADE,
    item_id BIGINT NOT NULL CHECK (item_id > 0),
    score_id BIGINT NOT NULL REFERENCES scores ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    total_score BIGINT NOT NULL CHECK (total_score >= 0),
    new_rank INTEGER CHECK (new_rank > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at TIMESTAMPTZ
);
CREATE INDEX lazer_room_pending_events ON lazer_room_score_events(id) WHERE delivered_at IS NULL;
