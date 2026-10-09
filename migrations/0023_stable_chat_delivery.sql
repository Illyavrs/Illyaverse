-- Stable has no message acknowledgement. This records inclusion in a committed
-- server response, not client receipt or the shared website/lazer read marker.
-- Pre-cutover offline history has no reliable stable delivery state. Keep it
-- available through the existing history APIs without replaying an old archive.
ALTER TABLE protocol_private_messages ADD COLUMN stable_delivery_state TEXT NOT NULL DEFAULT 'legacy'
    CHECK(stable_delivery_state IN ('legacy','pending','delivered'));

-- Preserve only the old transport's provably unconsumed active-session tail.
UPDATE protocol_private_messages m SET stable_delivery_state='pending'
FROM bancho_sessions b
WHERE b.user_id=m.recipient_id AND b.expires_at>clock_timestamp() AND m.id>b.last_dm_id;

-- Every original shared PM writer now queues once, including messages accepted
-- before a player's first stable login. UUID retries reuse the existing row.
ALTER TABLE protocol_private_messages ALTER COLUMN stable_delivery_state SET DEFAULT 'pending';
CREATE INDEX protocol_private_stable_pending ON protocol_private_messages(recipient_id,id)
    WHERE stable_delivery_state='pending';
COMMENT ON COLUMN protocol_private_messages.stable_delivery_state IS
    'legacy=pre-cutover unknown; pending=not included in a stable response; delivered=included in committed stable response, not read or network-acknowledged';
