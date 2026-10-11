-- Staff broadcasts (website notification and/or in-game chat) and website notifications for private
-- messages. A private-message notification stores only who wrote, never the message body.
CREATE TABLE broadcasts (
    id BIGSERIAL PRIMARY KEY,
    sender_id BIGINT NOT NULL REFERENCES users(id),
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    message TEXT NOT NULL CHECK (char_length(message) BETWEEN 1 AND 800),
    to_web BOOLEAN NOT NULL,
    to_game BOOLEAN NOT NULL,
    recipients INTEGER NOT NULL DEFAULT 0 CHECK (recipients >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CHECK (to_web OR to_game)
);
CREATE INDEX broadcasts_recent ON broadcasts(created_at DESC);

ALTER TABLE user_notifications
    ADD COLUMN broadcast_id BIGINT REFERENCES broadcasts(id) ON DELETE CASCADE,
    ADD COLUMN sender_id BIGINT REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE user_notifications
    DROP CONSTRAINT user_notifications_kind_check,
    DROP CONSTRAINT user_notifications_check;
ALTER TABLE user_notifications
    ADD CONSTRAINT user_notifications_kind_check CHECK (kind IN
        ('rank_request_approved', 'rank_request_rejected', 'announcement_published',
         'private_message', 'broadcast')),
    ADD CONSTRAINT user_notifications_check CHECK (
        (kind IN ('rank_request_approved', 'rank_request_rejected')
            AND rank_request_id IS NOT NULL AND announcement_id IS NULL AND publication_at IS NULL
            AND broadcast_id IS NULL AND sender_id IS NULL)
        OR
        (kind = 'announcement_published' AND rank_request_id IS NULL
            AND announcement_id IS NOT NULL AND publication_at IS NOT NULL AND message IS NULL
            AND broadcast_id IS NULL AND sender_id IS NULL)
        OR
        (kind = 'private_message' AND sender_id IS NOT NULL AND message IS NULL
            AND rank_request_id IS NULL AND announcement_id IS NULL AND publication_at IS NULL
            AND broadcast_id IS NULL)
        OR
        (kind = 'broadcast' AND broadcast_id IS NOT NULL AND message IS NOT NULL
            AND rank_request_id IS NULL AND announcement_id IS NULL AND publication_at IS NULL
            AND sender_id IS NULL)
    );

CREATE OR REPLACE FUNCTION illya_preserve_notification_identity() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF ROW(NEW.id, NEW.identity, NEW.user_id, NEW.event_key, NEW.kind,
        NEW.rank_request_id, NEW.announcement_id, NEW.publication_at,
        NEW.title, NEW.message, NEW.created_at, NEW.broadcast_id, NEW.sender_id) IS DISTINCT FROM
       ROW(OLD.id, OLD.identity, OLD.user_id, OLD.event_key, OLD.kind,
        OLD.rank_request_id, OLD.announcement_id, OLD.publication_at,
        OLD.title, OLD.message, OLD.created_at, OLD.broadcast_id, OLD.sender_id) THEN
        RAISE EXCEPTION USING ERRCODE = '23514',
            MESSAGE = 'notification event identity and content are immutable';
    END IF;
    RETURN NEW;
END;
$$;

-- One notification per sender every ten minutes, whichever client (stable, lazer or website) sent it.
CREATE FUNCTION illya_notify_private_message() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.sender_id = NEW.recipient_id THEN
        RETURN NEW;
    END IF;
    INSERT INTO user_notifications(user_id, event_key, kind, sender_id, title)
    SELECT NEW.recipient_id,
           'pm:' || NEW.sender_id || ':' || (extract(epoch FROM NEW.created_at)::bigint / 600),
           'private_message', NEW.sender_id, sender.username
    FROM users sender
    WHERE sender.id = NEW.sender_id
      AND EXISTS (SELECT 1 FROM users r WHERE r.id = NEW.recipient_id AND NOT r.is_restricted)
    ON CONFLICT (user_id, event_key) DO NOTHING;
    RETURN NEW;
END;
$$;
CREATE TRIGGER protocol_private_messages_notify
    AFTER INSERT ON protocol_private_messages
    FOR EACH ROW EXECUTE FUNCTION illya_notify_private_message();

-- In-game delivery: broadcasts are posted to this channel, which every client can read.
INSERT INTO channels(name, description) VALUES ('announce', 'Server announcements')
ON CONFLICT (name) DO NOTHING;

UPDATE roles SET permissions = array_append(permissions, 'broadcast.send')
WHERE name = 'moderator' AND NOT ('broadcast.send' = ANY(permissions));
