-- Notifications are emitted by new decisions/publications, never backfilled.
-- Source identities determine every link; no arbitrary URL or private PM body
-- is stored in this table.
CREATE TABLE user_notifications (
    id BIGSERIAL PRIMARY KEY CHECK (id > 0),
    -- This identity survives restoration. A reused sequence ID must not match
    -- an acknowledgement issued for an older, different row.
    identity UUID NOT NULL UNIQUE DEFAULT gen_random_uuid(),
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_key TEXT NOT NULL CHECK (char_length(event_key) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (kind IN
        ('rank_request_approved', 'rank_request_rejected', 'announcement_published')),
    rank_request_id BIGINT REFERENCES beatmap_rank_requests(id) ON DELETE CASCADE,
    announcement_id BIGINT REFERENCES announcements(id) ON DELETE CASCADE,
    publication_at TIMESTAMPTZ,
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 600),
    message TEXT CHECK (char_length(message) <= 1000 AND octet_length(message) <= 4000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    read_at TIMESTAMPTZ,
    UNIQUE (user_id, event_key),
    CHECK (
        (kind IN ('rank_request_approved', 'rank_request_rejected')
            AND rank_request_id IS NOT NULL AND announcement_id IS NULL AND publication_at IS NULL)
        OR
        (kind = 'announcement_published' AND rank_request_id IS NULL
            AND announcement_id IS NOT NULL AND publication_at IS NOT NULL AND message IS NULL)
    )
);
CREATE INDEX user_notifications_owner ON user_notifications(user_id, id DESC);
CREATE INDEX user_notifications_unread ON user_notifications(user_id, id DESC)
    WHERE read_at IS NULL;

CREATE FUNCTION illya_preserve_notification_identity() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF ROW(NEW.id, NEW.identity, NEW.user_id, NEW.event_key, NEW.kind,
        NEW.rank_request_id, NEW.announcement_id, NEW.publication_at,
        NEW.title, NEW.message, NEW.created_at) IS DISTINCT FROM
       ROW(OLD.id, OLD.identity, OLD.user_id, OLD.event_key, OLD.kind,
        OLD.rank_request_id, OLD.announcement_id, OLD.publication_at,
        OLD.title, OLD.message, OLD.created_at) THEN
        RAISE EXCEPTION USING ERRCODE = '23514',
            MESSAGE = 'notification event identity and content are immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER user_notifications_identity
    BEFORE UPDATE ON user_notifications FOR EACH ROW
    EXECUTE FUNCTION illya_preserve_notification_identity();
