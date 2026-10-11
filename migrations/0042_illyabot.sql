-- IllyaBot: the server's chat bot. It is an ordinary users row so every client can render its
-- messages, but it cannot sign in and is left out of counts and rankings.
ALTER TABLE users ADD COLUMN is_bot BOOLEAN NOT NULL DEFAULT FALSE;

-- Human accounts stay below 1,000,000; ids from there up are reserved for system accounts.
ALTER SEQUENCE users_id_seq MAXVALUE 999999;

INSERT INTO users(id, username, email, password_hash, password_md5_hash, country, bio, accent_color, is_bot)
VALUES (1000000, 'IllyaBot', 'illyabot@illyaverse.invalid', '!', '!', 'TW',
        'Illyaverse 的系統機器人。輸入 !help 查看指令。', '#f19ac0', TRUE);

-- Command cursors, so a restart neither replays old commands nor misses new ones.
CREATE TABLE bot_state (key TEXT PRIMARY KEY, value BIGINT NOT NULL);

-- The beatmap a player last shared with /np, which !with, !maplink and friends refer to.
CREATE TABLE bot_np (
    user_id BIGINT PRIMARY KEY REFERENCES users ON DELETE CASCADE,
    beatmap_id BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Multiplayer room management through the bot.
CREATE TABLE stable_room_referees (
    room_id SMALLINT NOT NULL REFERENCES stable_rooms ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    PRIMARY KEY (room_id, user_id)
);
CREATE TABLE stable_room_timers (
    room_id SMALLINT PRIMARY KEY REFERENCES stable_rooms ON DELETE CASCADE,
    due_at TIMESTAMPTZ NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('start', 'notice'))
);
