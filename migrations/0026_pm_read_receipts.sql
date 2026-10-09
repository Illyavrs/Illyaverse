-- Sequence allocation order is not transaction commit order. A PM read marker
-- remains a native/API compatibility cursor; unread state names actual rows.
CREATE TABLE pm_read_receipts (
    reader_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    message_id BIGINT NOT NULL REFERENCES chat_messages ON DELETE CASCADE,
    read_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(reader_id,message_id)
);
CREATE INDEX pm_read_receipts_message ON pm_read_receipts(message_id);

CREATE FUNCTION illya_pm_read_receipts() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- Equal markers must take another committed-row snapshot: a lower sequence
    -- ID can become visible after the earlier acknowledgement.
    IF NEW.last_read_id < OLD.last_read_id THEN RETURN NEW; END IF;
    INSERT INTO pm_read_receipts(reader_id,message_id)
    SELECT NEW.user_id,m.id
    FROM channels c
    JOIN users peer ON peer.id=CASE WHEN c.dm_user_low=NEW.user_id THEN c.dm_user_high ELSE c.dm_user_low END
    JOIN chat_messages m ON m.channel_id=c.id
    JOIN users sender ON sender.id=m.user_id
    WHERE c.id=NEW.channel_id AND c.kind='PM'
      AND NEW.user_id IN(c.dm_user_low,c.dm_user_high)
      AND m.id<=NEW.last_read_id AND m.user_id<>NEW.user_id
      AND NOT peer.is_restricted AND NOT sender.is_restricted
      AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block'
        AND ((r.user_id=NEW.user_id AND r.target_id IN(peer.id,m.user_id))
          OR (r.target_id=NEW.user_id AND r.user_id IN(peer.id,m.user_id))))
    ON CONFLICT DO NOTHING;
    RETURN NEW;
END $$;
CREATE TRIGGER pm_read_receipts_marker AFTER UPDATE OF last_read_id ON lazer_channel_members
    FOR EACH ROW EXECUTE FUNCTION illya_pm_read_receipts();

-- Preserve the legacy classification of rows currently visible at cutover.
-- A legacy cursor cannot establish whether an already committed lower-ID row
-- was observed before upgrade, so this is not a repair of pre-upgrade races.
INSERT INTO pm_read_receipts(reader_id,message_id)
SELECT member.user_id,m.id
FROM lazer_channel_members member
JOIN channels c ON c.id=member.channel_id AND c.kind='PM'
JOIN users peer ON peer.id=CASE WHEN c.dm_user_low=member.user_id THEN c.dm_user_high ELSE c.dm_user_low END
JOIN chat_messages m ON m.channel_id=c.id
JOIN users sender ON sender.id=m.user_id
WHERE member.user_id IN(c.dm_user_low,c.dm_user_high)
  AND m.id<=member.last_read_id AND m.user_id<>member.user_id
  AND NOT peer.is_restricted AND NOT sender.is_restricted
  AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block'
    AND ((r.user_id=member.user_id AND r.target_id IN(peer.id,m.user_id))
      OR (r.target_id=member.user_id AND r.user_id IN(peer.id,m.user_id))))
ON CONFLICT DO NOTHING;

COMMENT ON TABLE pm_read_receipts IS
    'Durable visible incoming PM row acknowledgements. Independent of numeric cursor commit order and membership retention; no reconstruction of pre-upgrade observation.';
COMMENT ON COLUMN lazer_channel_members.last_read_id IS
    'Monotonic native/API message cursor. PM unread state uses pm_read_receipts, not this sequence watermark.';
