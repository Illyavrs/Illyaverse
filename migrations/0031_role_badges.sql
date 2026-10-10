-- Role badges: osu-web style group labels with an operator-chosen colour.
ALTER TABLE roles
    ADD COLUMN short_name TEXT NOT NULL DEFAULT '' CHECK (char_length(short_name) <= 8),
    ADD COLUMN colour TEXT CHECK (colour IS NULL OR colour ~ '^#[0-9a-f]{6}$'),
    ADD COLUMN display_badge BOOLEAN NOT NULL DEFAULT false;
UPDATE roles SET short_name='ADMIN', colour='#ff5c8d', display_badge=true WHERE name='administrator';
UPDATE roles SET short_name='MOD', colour='#3b82f6', display_badge=true WHERE name='moderator';
