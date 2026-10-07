-- Directed relationships shared by stable, lazer, and the website.
CREATE TABLE user_relationships (
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    target_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    relation_type TEXT NOT NULL CHECK (relation_type IN ('friend','block')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id,target_id),
    CHECK (user_id <> target_id)
);
CREATE INDEX relationships_target ON user_relationships(target_id,user_id);
