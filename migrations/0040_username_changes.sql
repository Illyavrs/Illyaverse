-- Username changes: players may rename once per cooldown, and a released name stays reserved for the
-- previous owner for a while so nobody can take over a name that others still associate with them.
ALTER TABLE users ADD COLUMN username_changed_at TIMESTAMPTZ;
CREATE TABLE username_history (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    old_username TEXT NOT NULL,
    new_username TEXT NOT NULL,
    changed_by BIGINT REFERENCES users ON DELETE SET NULL,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX username_history_old_lower ON username_history (lower(old_username), changed_at DESC);
CREATE INDEX username_history_user ON username_history (user_id, changed_at DESC);
