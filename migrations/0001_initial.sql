CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    username TEXT NOT NULL,
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    password_md5_hash TEXT NOT NULL,
    country TEXT NOT NULL DEFAULT 'TW',
    bio TEXT NOT NULL DEFAULT '',
    avatar_url TEXT,
    banner_url TEXT,
    accent_color TEXT NOT NULL DEFAULT '#f19ac0',
    preferences JSONB NOT NULL DEFAULT '{}',
    is_restricted BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_username_unique ON users (lower(username));
CREATE UNIQUE INDEX users_email_unique ON users (lower(email));
CREATE TABLE roles (id BIGSERIAL PRIMARY KEY, name TEXT UNIQUE NOT NULL, description TEXT NOT NULL DEFAULT '', permissions TEXT[] NOT NULL DEFAULT '{}');
CREATE TABLE user_roles (user_id BIGINT REFERENCES users ON DELETE CASCADE, role_id BIGINT REFERENCES roles ON DELETE CASCADE, PRIMARY KEY(user_id,role_id));
INSERT INTO roles(name,description,permissions) VALUES
('player','Community player',ARRAY['chat.send','beatmaps.upload']),
('moderator','Community moderation',ARRAY['chat.send','beatmaps.upload','beatmaps.import','beatmaps.moderate','users.moderate','scores.moderate','audit.read','admin.view']),
('administrator','Server administration',ARRAY['*']);
CREATE TABLE sessions (id UUID PRIMARY KEY, user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE, expires_at TIMESTAMPTZ NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE INDEX sessions_expiry ON sessions(expires_at);
CREATE TABLE beatmaps (
    id BIGSERIAL PRIMARY KEY,
    official_id BIGINT UNIQUE,
    official_set_id BIGINT,
    uploaded_by BIGINT REFERENCES users,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    creator TEXT NOT NULL,
    version TEXT NOT NULL,
    mode SMALLINT NOT NULL CHECK(mode BETWEEN 0 AND 3),
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','ranked','loved','unranked')),
    source TEXT NOT NULL DEFAULT 'local' CHECK(source IN ('local','official')),
    stars DOUBLE PRECISION NOT NULL,
    bpm DOUBLE PRECISION NOT NULL DEFAULT 0,
    length_seconds INTEGER NOT NULL DEFAULT 0,
    max_combo INTEGER NOT NULL DEFAULT 0,
    md5 TEXT UNIQUE NOT NULL,
    osu_path TEXT NOT NULL,
    archive_path TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX beatmaps_source_status ON beatmaps(source,status);
CREATE TABLE scores (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users,
    beatmap_id BIGINT NOT NULL REFERENCES beatmaps,
    mode SMALLINT NOT NULL CHECK(mode BETWEEN 0 AND 3),
    score BIGINT NOT NULL CHECK(score >= 0),
    accuracy DOUBLE PRECISION NOT NULL CHECK(accuracy BETWEEN 0 AND 100),
    max_combo INTEGER NOT NULL CHECK(max_combo >= 0),
    mods BIGINT NOT NULL DEFAULT 0 CHECK(mods >= 0),
    pp DOUBLE PRECISION NOT NULL CHECK(pp >= 0),
    passed BOOLEAN NOT NULL DEFAULT TRUE,
    verification TEXT NOT NULL DEFAULT 'pending' CHECK(verification IN ('pending','verified','rejected')),
    hit_counts JSONB NOT NULL DEFAULT '{}',
    replay_path TEXT,
    client TEXT NOT NULL DEFAULT 'api',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX scores_user ON scores(user_id,created_at DESC);
CREATE INDEX scores_map ON scores(beatmap_id,pp DESC);
CREATE TABLE channels (id BIGSERIAL PRIMARY KEY,name TEXT UNIQUE NOT NULL,description TEXT NOT NULL DEFAULT '',required_permission TEXT,created_at TIMESTAMPTZ NOT NULL DEFAULT now());
INSERT INTO channels(name,description) VALUES ('general','Illyaverse community'),('mapping','Mapping and beatmap discussion'),('help','Connection and technical support');
INSERT INTO channels(name,description,required_permission) VALUES ('staff','Moderation coordination','admin.view');
CREATE TABLE chat_messages (id BIGSERIAL PRIMARY KEY,channel_id BIGINT NOT NULL REFERENCES channels ON DELETE CASCADE,user_id BIGINT NOT NULL REFERENCES users,content TEXT NOT NULL CHECK(length(content) BETWEEN 1 AND 1000),created_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE INDEX chat_channel_id ON chat_messages(channel_id,id DESC);
CREATE TABLE audit_events (id BIGSERIAL PRIMARY KEY,actor_id BIGINT REFERENCES users,action TEXT NOT NULL,target_type TEXT NOT NULL,target_id TEXT,details JSONB NOT NULL DEFAULT '{}',created_at TIMESTAMPTZ NOT NULL DEFAULT now());
