-- Relax has a separate score/PP/statistics partition, not a new native ruleset.
ALTER TABLE scores ADD COLUMN score_variant text NOT NULL DEFAULT 'vanilla';
ALTER TABLE scores ADD COLUMN pp_calculation_version text
    CHECK (pp_calculation_version IS NULL OR length(pp_calculation_version) BETWEEN 1 AND 80);

-- The legacy projection was validated at ingestion and is retained for every
-- source. Historical mania RX stays readable as relax but never grants PP.
UPDATE scores SET score_variant='relax' WHERE (mods & 128)<>0;
UPDATE scores SET pp_eligible=false
WHERE (mode=3 AND score_variant='relax')
   OR (mods & 10240)<>0
   OR COALESCE(api_mods @> '[{"acronym":"DA"}]'::jsonb, false);

-- RX's zero legacy score multiplier does not define a common score scale.
-- Invalidate only derived display caches, retaining raw scores, native context,
-- map-derived counts, PP, replay ownership and moderation history. A non-null
-- version keeps the ordinary historical conversion worker from retrying these.
UPDATE scores SET standardised_score=NULL, standardised_without_mods=NULL,
    classic_score=NULL, score_scale_status='unavailable',
    score_scale_version='relax_score_scale.unavailable.v1',
    score_scale_reason='RX uses a separate PP leaderboard; a common score scale is unavailable.'
WHERE score_variant='relax';

-- Existing false eligibility has no provenance: it may represent RX's previous
-- automatic exclusion or an explicit operator exclusion. Never silently promote
-- it here. An audited, bounded recalculation must validate historical mods and
-- calculate their PP before the operator enables an existing RX score.
ALTER TABLE scores
    ADD CONSTRAINT scores_variant_known CHECK (score_variant IN ('vanilla','relax')),
    ADD CONSTRAINT scores_variant_mod_projection CHECK (
        (score_variant='relax') = ((mods & 128)<>0)
    );

CREATE INDEX scores_user_variant_mode
    ON scores(user_id,score_variant,mode,created_at DESC,id DESC);
CREATE INDEX scores_map_variant_mode_pp
    ON scores(beatmap_id,score_variant,mode,pp DESC,id)
    WHERE verification='verified' AND passed AND pp_eligible;
CREATE INDEX scores_variant_mode_user_best
    ON scores(score_variant,mode,user_id,beatmap_id,pp DESC,id)
    WHERE verification='verified' AND passed AND pp_eligible;
