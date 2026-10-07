-- Raw scores remain immutable. Conversion provenance does not grant verification.
ALTER TABLE scores
    ADD COLUMN native_submission jsonb,
    ADD COLUMN standardised_score bigint CHECK (standardised_score >= 0),
    ADD COLUMN standardised_without_mods bigint CHECK (standardised_without_mods >= 0),
    ADD COLUMN classic_score bigint CHECK (classic_score >= 0),
    ADD COLUMN native_basic_maximum integer CHECK (native_basic_maximum >= 0),
    ADD COLUMN score_scale_status text NOT NULL DEFAULT 'unavailable'
        CHECK (score_scale_status IN ('native_reported','legacy_estimated','legacy_scorev2','unavailable')),
    ADD COLUMN score_scale_version text,
    ADD COLUMN score_scale_reason text,
    ADD COLUMN legacy_attributes jsonb;

ALTER TABLE scores
    ADD CONSTRAINT scores_native_submission_object CHECK (
        native_submission IS NULL OR (client='lazer' AND jsonb_typeof(native_submission)='object')
    ),
    ADD CONSTRAINT scores_legacy_attributes_object CHECK (
        legacy_attributes IS NULL OR jsonb_typeof(legacy_attributes)='object'
    ),
    ADD CONSTRAINT scores_scale_reason_length CHECK (
        score_scale_reason IS NULL OR length(score_scale_reason)<=512
    ),
    ADD CONSTRAINT scores_scale_source CHECK (
        (score_scale_status<>'native_reported' OR client='lazer') AND
        (score_scale_status NOT IN ('legacy_estimated','legacy_scorev2') OR client='stable')
    ),
    ADD CONSTRAINT scores_scale_values CHECK (
        (score_scale_status='unavailable' AND standardised_score IS NULL AND classic_score IS NULL) OR
        (score_scale_status<>'unavailable' AND standardised_score IS NOT NULL AND classic_score IS NOT NULL)
    );

-- Retain the original native context independently of expiring submission tokens.
-- The background worker validates its map-derived counts before computing caches.
UPDATE scores s SET native_submission=t.submission
FROM lazer_score_tokens t
WHERE t.score_id=s.id AND s.client='lazer' AND jsonb_typeof(t.submission)='object';

CREATE INDEX scores_scale_recovery ON scores(id)
    WHERE score_scale_status='unavailable' AND score_scale_version IS NULL;
CREATE INDEX scores_standardised_map ON scores(beatmap_id,mode,standardised_score DESC,id)
    WHERE standardised_score IS NOT NULL AND verification='verified' AND passed;
