-- Existing text stays Markdown. Formats are explicit data, never inferred from content.
ALTER TABLE users ADD COLUMN bio_format TEXT NOT NULL DEFAULT 'markdown'
    CHECK (bio_format IN ('markdown','bbcode'));
ALTER TABLE announcements ADD COLUMN body_format TEXT NOT NULL DEFAULT 'markdown'
    CHECK (body_format IN ('markdown','bbcode'));
ALTER TABLE beatmap_rank_requests ADD COLUMN reason_format TEXT NOT NULL DEFAULT 'markdown'
    CHECK (reason_format IN ('markdown','bbcode'));
ALTER TABLE beatmap_rank_requests ADD COLUMN review_reason_format TEXT NOT NULL DEFAULT 'markdown'
    CHECK (review_reason_format IN ('markdown','bbcode'));

-- Rich-text reasons can contain paragraphs and tabs; other controls stay forbidden.
ALTER TABLE beatmap_rank_requests DROP CONSTRAINT beatmap_rank_requests_reason_check;
ALTER TABLE beatmap_rank_requests ADD CONSTRAINT beatmap_rank_requests_reason_check CHECK (
    reason = btrim(reason) AND char_length(reason) BETWEEN 1 AND 1000
    AND octet_length(reason) <= 4000
    AND translate(reason, E'\n\t', '') !~ '[[:cntrl:]]');
ALTER TABLE beatmap_rank_requests DROP CONSTRAINT beatmap_rank_requests_review_reason_check;
ALTER TABLE beatmap_rank_requests ADD CONSTRAINT beatmap_rank_requests_review_reason_check CHECK (
    review_reason = btrim(review_reason) AND char_length(review_reason) BETWEEN 1 AND 1000
    AND octet_length(review_reason) <= 4000
    AND translate(review_reason, E'\n\t', '') !~ '[[:cntrl:]]');
ALTER TABLE beatmap_status_overrides DROP CONSTRAINT beatmap_status_overrides_reason_check;
ALTER TABLE beatmap_status_overrides ADD CONSTRAINT beatmap_status_overrides_reason_check CHECK (
    reason = btrim(reason) AND char_length(reason) BETWEEN 1 AND 1000
    AND octet_length(reason) <= 4000
    AND translate(reason, E'\n\t', '') !~ '[[:cntrl:]]');
