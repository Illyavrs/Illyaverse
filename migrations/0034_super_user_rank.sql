-- Role ranks decide who may manage whom: nobody can change a user or role at or above their own rank.
-- The built-in super_user sits above every other role and may be held by exactly one user.
ALTER TABLE roles ADD COLUMN rank INTEGER NOT NULL DEFAULT 10 CHECK (rank BETWEEN 0 AND 1000);
UPDATE roles SET rank=0 WHERE name='player';
UPDATE roles SET rank=50 WHERE name='beatmap_nominator';
UPDATE roles SET rank=100 WHERE name='moderator';
UPDATE roles SET rank=500 WHERE name='administrator';
INSERT INTO roles(name,description,permissions,rank,short_name,colour,display_badge)
VALUES ('super_user','Server owner with authority over every other role',ARRAY['*'],1000,'SUPER','#f5b301',true);

CREATE FUNCTION illya_single_super_user() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.role_id = (SELECT id FROM roles WHERE name='super_user') THEN
        PERFORM pg_advisory_xact_lock(hashtext('illya_single_super_user'));
        IF EXISTS (SELECT 1 FROM user_roles WHERE role_id=NEW.role_id AND user_id<>NEW.user_id) THEN
            RAISE EXCEPTION 'A server can have only one super_user' USING ERRCODE='23505';
        END IF;
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER user_roles_single_super_user
    BEFORE INSERT OR UPDATE ON user_roles
    FOR EACH ROW EXECUTE FUNCTION illya_single_super_user();
