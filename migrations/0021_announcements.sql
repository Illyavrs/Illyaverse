-- Website announcements are published explicitly by an authorized operator.
CREATE TABLE announcements (
    id BIGSERIAL PRIMARY KEY,
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 160),
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 20000 AND octet_length(body) <= 65536),
    status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'archived')),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_by BIGINT NOT NULL REFERENCES users(id),
    updated_by BIGINT NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    published_at TIMESTAMPTZ,
    CHECK (status <> 'published' OR published_at IS NOT NULL)
);
CREATE INDEX announcements_published ON announcements (published_at DESC, id DESC)
    WHERE status = 'published';
CREATE INDEX announcements_updated ON announcements (updated_at DESC, id DESC);

