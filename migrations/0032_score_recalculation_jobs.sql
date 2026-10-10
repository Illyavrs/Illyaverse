-- Operator-started bulk pp recalculation: a job with its filter, progress and per-score outcomes.
CREATE TABLE score_recalculation_jobs (
    id BIGSERIAL PRIMARY KEY,
    created_by BIGINT NOT NULL REFERENCES users,
    filters JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','running','completed','failed','cancelled')),
    total BIGINT NOT NULL CHECK (total >= 0),
    processed BIGINT NOT NULL DEFAULT 0 CHECK (processed >= 0),
    updated BIGINT NOT NULL DEFAULT 0 CHECK (updated >= 0),
    unchanged BIGINT NOT NULL DEFAULT 0 CHECK (unchanged >= 0),
    skipped BIGINT NOT NULL DEFAULT 0 CHECK (skipped >= 0),
    failed BIGINT NOT NULL DEFAULT 0 CHECK (failed >= 0),
    last_score_id BIGINT NOT NULL DEFAULT 0,
    cancel_requested BOOLEAN NOT NULL DEFAULT false,
    error TEXT CHECK (error IS NULL OR length(error) <= 500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);
-- At most one job is queued or running; a new request must wait or cancel it.
CREATE UNIQUE INDEX score_recalculation_one_active ON score_recalculation_jobs ((true)) WHERE status IN ('queued','running');
CREATE INDEX score_recalculation_jobs_recent ON score_recalculation_jobs (id DESC);
-- Only scores whose pp changed, were skipped or failed are listed; unchanged ones are counted.
CREATE TABLE score_recalculation_items (
    job_id BIGINT NOT NULL REFERENCES score_recalculation_jobs ON DELETE CASCADE,
    score_id BIGINT NOT NULL REFERENCES scores ON DELETE CASCADE,
    outcome TEXT NOT NULL CHECK (outcome IN ('updated','skipped','failed')),
    previous_pp DOUBLE PRECISION,
    new_pp DOUBLE PRECISION,
    detail TEXT CHECK (detail IS NULL OR length(detail) <= 300),
    PRIMARY KEY (job_id, score_id)
);
