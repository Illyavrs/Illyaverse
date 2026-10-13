-- Decorations have no gameplay, score, or ranking effects. Catalog entries are
-- created by administrators; no unreviewed third-party names are seeded.
CREATE TABLE reward_catalog (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 32 AND name=btrim(name) AND name !~ '[[:cntrl:]]'),
    description TEXT NOT NULL DEFAULT '' CHECK (char_length(description) <= 300),
    kind TEXT NOT NULL CHECK (kind IN ('title', 'avatar_frame')),
    price BIGINT NOT NULL CHECK (price BETWEEN 1 AND 100000),
    color TEXT CHECK (color IS NULL OR color ~ '^#[0-9A-Fa-f]{6}$'),
    style TEXT,
    active BOOLEAN NOT NULL DEFAULT FALSE,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CHECK ((kind='title' AND style IS NULL)
        OR (kind='avatar_frame' AND style IS NOT NULL AND style IN ('line', 'double', 'corners')))
);
CREATE INDEX reward_catalog_active ON reward_catalog(active, id);

CREATE TABLE reward_wallets (
    user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    balance BIGINT NOT NULL DEFAULT 0 CHECK (balance >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE reward_mission_claims (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    week_start TIMESTAMPTZ NOT NULL,
    mission_key TEXT NOT NULL CHECK (mission_key IN ('play_5', 'maps_3', 'variety_2')),
    amount BIGINT NOT NULL DEFAULT 20 CHECK (amount=20),
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(user_id, week_start, mission_key),
    CHECK (week_start=(date_trunc('week',week_start AT TIME ZONE 'Asia/Taipei') AT TIME ZONE 'Asia/Taipei'))
);

CREATE TABLE reward_inventory (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reward_id BIGINT NOT NULL REFERENCES reward_catalog(id),
    acquired_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    paid BIGINT NOT NULL CHECK (paid BETWEEN 1 AND 100000),
    PRIMARY KEY(user_id, reward_id)
);
CREATE INDEX reward_inventory_reward ON reward_inventory(reward_id);

CREATE TABLE reward_equipment (
    user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    title_id BIGINT,
    avatar_frame_id BIGINT,
    FOREIGN KEY(user_id, title_id) REFERENCES reward_inventory(user_id, reward_id),
    FOREIGN KEY(user_id, avatar_frame_id) REFERENCES reward_inventory(user_id, reward_id)
);

CREATE TABLE reward_ledger (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    delta BIGINT NOT NULL CHECK (delta <> 0),
    balance_after BIGINT NOT NULL CHECK (balance_after >= 0),
    event_type TEXT NOT NULL CHECK (event_type IN ('mission', 'purchase')),
    mission_key TEXT CHECK (mission_key IS NULL OR mission_key IN ('play_5', 'maps_3', 'variety_2')),
    week_start TIMESTAMPTZ,
    reward_id BIGINT REFERENCES reward_catalog(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CHECK ((event_type='mission' AND delta=20 AND mission_key IS NOT NULL AND week_start IS NOT NULL AND reward_id IS NULL)
        OR (event_type='purchase' AND delta BETWEEN -100000 AND -1 AND reward_id IS NOT NULL AND mission_key IS NULL AND week_start IS NULL))
);
CREATE UNIQUE INDEX reward_ledger_mission_once
    ON reward_ledger(user_id, week_start, mission_key) WHERE event_type='mission';
CREATE UNIQUE INDEX reward_ledger_purchase_once
    ON reward_ledger(user_id, reward_id) WHERE event_type='purchase';
CREATE INDEX reward_ledger_user ON reward_ledger(user_id, id DESC);

CREATE INDEX scores_reward_progress ON scores(user_id, created_at)
    WHERE verification='verified' AND passed;
