-- Time spent on each play, counted like the game servers do (see record_play_seconds in sanctuary).
-- NULL means unknown, for example a failed stable play from before the client time was recorded.
ALTER TABLE scores ADD COLUMN play_seconds INTEGER CONSTRAINT scores_play_seconds_range CHECK (play_seconds BETWEEN 0 AND 36000);

-- lazer: time between the score token being issued and the score arriving, capped at the map length.
UPDATE scores s SET play_seconds=GREATEST(0,LEAST(
    CEIL(b.length_seconds/CASE WHEN s.mods&(64|512)<>0 THEN 1.5 WHEN s.mods&256<>0 THEN 0.75 ELSE 1 END)::int,
    EXTRACT(EPOCH FROM (s.created_at-t.created_at))::int))
FROM beatmaps b, lazer_score_tokens t
WHERE s.client='lazer' AND b.id=s.beatmap_id AND s.ingestion_key LIKE 'lazer:%'
  AND t.id=NULLIF(split_part(s.ingestion_key,':',2),'')::bigint AND s.play_seconds IS NULL;

-- Everything else that passed necessarily played the whole (rate-adjusted) map: a lower bound, not a guess.
UPDATE scores s SET play_seconds=LEAST(36000,CEIL(b.length_seconds/CASE WHEN s.mods&(64|512)<>0 THEN 1.5 WHEN s.mods&256<>0 THEN 0.75 ELSE 1 END)::int)
FROM beatmaps b
WHERE b.id=s.beatmap_id AND s.passed AND s.play_seconds IS NULL;
