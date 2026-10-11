-- The mode and score variant a profile opens with. Defaults match the previous fixed behaviour.
ALTER TABLE users
  ADD COLUMN preferred_mode SMALLINT NOT NULL DEFAULT 0 CONSTRAINT users_preferred_mode_range CHECK (preferred_mode BETWEEN 0 AND 3),
  ADD COLUMN preferred_variant TEXT NOT NULL DEFAULT 'vanilla' CONSTRAINT users_preferred_variant_known CHECK (preferred_variant IN ('vanilla','relax')),
  ADD CONSTRAINT users_preferred_partition CHECK (NOT (preferred_mode = 3 AND preferred_variant = 'relax'));
