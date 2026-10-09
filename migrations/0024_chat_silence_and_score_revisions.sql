ALTER TABLE users
    ADD COLUMN chat_silenced_until TIMESTAMPTZ,
    ADD COLUMN chat_silence_revision BIGINT NOT NULL DEFAULT 0
        CHECK (chat_silence_revision >= 0);

ALTER TABLE bancho_sessions ADD COLUMN last_chat_silence_until TIMESTAMPTZ;

ALTER TABLE scores ADD COLUMN moderation_revision BIGINT NOT NULL DEFAULT 0
    CHECK (moderation_revision >= 0);

-- Resource revisions belong to the database, so API, native publication and
-- recovery writers cannot silently reset a revision or allow an ABA decision.
CREATE FUNCTION illya_advance_chat_silence_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.chat_silenced_until IS DISTINCT FROM OLD.chat_silenced_until THEN
        NEW.chat_silence_revision := OLD.chat_silence_revision + 1;
    ELSE
        NEW.chat_silence_revision := OLD.chat_silence_revision;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER users_chat_silence_revision
    BEFORE UPDATE ON users FOR EACH ROW
    EXECUTE FUNCTION illya_advance_chat_silence_revision();

CREATE FUNCTION illya_advance_score_moderation_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.verification IS DISTINCT FROM OLD.verification THEN
        NEW.moderation_revision := OLD.moderation_revision + 1;
    ELSE
        NEW.moderation_revision := OLD.moderation_revision;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER scores_moderation_revision
    BEFORE UPDATE ON scores FOR EACH ROW
    EXECUTE FUNCTION illya_advance_score_moderation_revision();

UPDATE roles SET permissions = array_append(permissions, 'chat.moderate')
WHERE name = 'moderator' AND NOT ('chat.moderate' = ANY(permissions));
