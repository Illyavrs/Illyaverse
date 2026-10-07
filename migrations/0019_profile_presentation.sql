ALTER TABLE users
    ADD COLUMN banner_position_x INTEGER NOT NULL DEFAULT 50
        CONSTRAINT users_banner_position_x_range CHECK (banner_position_x BETWEEN 0 AND 100),
    ADD COLUMN banner_position_y INTEGER NOT NULL DEFAULT 50
        CONSTRAINT users_banner_position_y_range CHECK (banner_position_y BETWEEN 0 AND 100);
