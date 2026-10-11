-- Badge icon chosen by staff; NULL keeps the built-in default for the role name.
ALTER TABLE roles ADD COLUMN icon TEXT CONSTRAINT roles_icon_key CHECK (icon IS NULL OR icon ~ '^[a-z][a-z-]{0,23}$');
