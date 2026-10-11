-- #announce carries staff broadcasts: everyone reads it, only the broadcast tool writes to it.
ALTER TABLE channels ADD COLUMN read_only BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE channels SET read_only = TRUE WHERE name = 'announce';

-- Lazer players join the public channels on first use; add the existing ones to #announce.
INSERT INTO lazer_channel_members(user_id, channel_id)
SELECT a.user_id, c.id FROM lazer_chat_activity a CROSS JOIN channels c WHERE c.name = 'announce'
ON CONFLICT DO NOTHING;

-- Stable sessions fixed their channel list at login; give live ones the new channel.
UPDATE bancho_sessions
SET channels = channels || (SELECT id FROM channels WHERE name = 'announce')
WHERE EXISTS (SELECT 1 FROM channels WHERE name = 'announce')
  AND NOT ((SELECT id FROM channels WHERE name = 'announce') = ANY(channels));
