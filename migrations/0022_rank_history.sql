-- Each UTC day retains its latest complete observation of all seven partitions.
-- Collection only replaces the current day; missed days have no synthetic rows.
CREATE TABLE rank_history_batches (
    date DATE PRIMARY KEY,
    captured_at TIMESTAMPTZ NOT NULL,
    row_count BIGINT NOT NULL CHECK (row_count >= 0),
    partition_count SMALLINT NOT NULL DEFAULT 7 CHECK (partition_count = 7),
    CONSTRAINT rank_history_batches_utc_date CHECK ((captured_at AT TIME ZONE 'UTC')::date = date)
);
CREATE TABLE rank_history_daily (
    user_id BIGINT NOT NULL REFERENCES users(id),
    date DATE NOT NULL REFERENCES rank_history_batches(date),
    mode SMALLINT NOT NULL,
    variant TEXT NOT NULL,
    captured_at TIMESTAMPTZ NOT NULL,
    country TEXT NOT NULL CHECK (country ~ '^[A-Z]{2}$'),
    pp DOUBLE PRECISION NOT NULL CHECK (pp >= 0 AND pp < 'Infinity'::double precision),
    rank BIGINT,
    country_rank BIGINT,
    PRIMARY KEY (user_id, mode, variant, date),
    CHECK ((variant = 'vanilla' AND mode BETWEEN 0 AND 3)
        OR (variant = 'relax' AND mode BETWEEN 0 AND 2)),
    CHECK ((captured_at AT TIME ZONE 'UTC')::date = date),
    CHECK ((pp = 0 AND rank IS NULL AND country_rank IS NULL)
        OR (pp > 0 AND rank > 0 AND country_rank > 0 AND country_rank <= rank
            AND rank IS NOT NULL AND country_rank IS NOT NULL))
);
CREATE INDEX rank_history_daily_date ON rank_history_daily(date);
