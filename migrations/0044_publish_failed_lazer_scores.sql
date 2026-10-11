-- Failed lazer plays have no replay, so they never reached the publication step that only runs
-- when a replay capture finishes. Publish the ones left waiting, unless a moderator has ruled on them.
WITH published AS (
    UPDATE scores s SET verification = 'verified'
    WHERE s.client = 'lazer' AND NOT s.passed AND s.verification = 'pending'
      AND NOT EXISTS (SELECT 1 FROM audit_events a WHERE a.action = 'scores.verify' AND a.target_type = 'score' AND a.target_id = s.id::text)
      AND EXISTS (SELECT 1 FROM users u WHERE u.id = s.user_id AND NOT u.is_restricted)
    RETURNING s.id, s.user_id
)
INSERT INTO audit_events(actor_id, action, target_type, target_id, details)
SELECT user_id, 'scores.native_publish', 'score', id::text,
       '{"client":"lazer","admission":"native_protocol","gameplay_verified":false,"backfill":"failed plays need no replay"}'::jsonb
FROM published;
