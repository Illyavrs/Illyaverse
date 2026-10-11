-- Per-locale group introductions, edited by staff next to the role. `description`
-- stays as the fallback for locales without a translation.
ALTER TABLE roles ADD COLUMN description_i18n JSONB NOT NULL DEFAULT '{}'::jsonb
  CONSTRAINT roles_description_i18n_object CHECK (jsonb_typeof(description_i18n) = 'object');
