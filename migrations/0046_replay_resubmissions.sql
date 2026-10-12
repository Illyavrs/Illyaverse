-- Replay uploads are claims that must be reviewed, never native admissions.
ALTER TABLE scores ADD COLUMN submission_origin TEXT NOT NULL DEFAULT 'client'
    CHECK (submission_origin IN ('client', 'replay'));
ALTER TABLE scores ADD COLUMN replay_played_at TIMESTAMPTZ;
CREATE INDEX scores_replay_review ON scores(user_id, verification)
    WHERE submission_origin = 'replay';
