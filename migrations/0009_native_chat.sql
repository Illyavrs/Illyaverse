-- Native notification events are emitted only when the writing transaction commits.
ALTER TABLE channels ADD COLUMN kind TEXT NOT NULL DEFAULT 'PUBLIC' CHECK(kind IN ('PUBLIC','PM'));
ALTER TABLE channels ADD COLUMN dm_user_low BIGINT REFERENCES users ON DELETE CASCADE;
ALTER TABLE channels ADD COLUMN dm_user_high BIGINT REFERENCES users ON DELETE CASCADE;
ALTER TABLE channels ADD CONSTRAINT channels_pm_pair CHECK(
    (kind='PUBLIC' AND dm_user_low IS NULL AND dm_user_high IS NULL) OR
    (kind='PM' AND dm_user_low IS NOT NULL AND dm_user_high IS NOT NULL AND dm_user_low<dm_user_high AND required_permission IS NOT DISTINCT FROM 'chat.pm'));
CREATE UNIQUE INDEX channels_pm_unique ON channels(dm_user_low,dm_user_high) WHERE kind='PM';
ALTER TABLE lazer_channel_members ADD COLUMN last_read_id BIGINT NOT NULL DEFAULT 0 CHECK(last_read_id>=0);
ALTER TABLE chat_messages ADD COLUMN is_action BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE chat_messages ADD COLUMN client_uuid TEXT CHECK(length(client_uuid)<=128);
ALTER TABLE protocol_private_messages ADD COLUMN is_action BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE protocol_private_messages ADD COLUMN client_uuid TEXT CHECK(length(client_uuid)<=128);
ALTER TABLE chat_messages ADD COLUMN private_message_id BIGINT UNIQUE REFERENCES protocol_private_messages ON DELETE CASCADE;
CREATE UNIQUE INDEX chat_message_retry ON chat_messages(user_id,client_uuid) WHERE client_uuid IS NOT NULL;
CREATE UNIQUE INDEX protocol_private_retry ON protocol_private_messages(sender_id,client_uuid) WHERE client_uuid IS NOT NULL;
CREATE TABLE lazer_chat_activity (
    user_id BIGINT PRIMARY KEY REFERENCES users ON DELETE CASCADE,
    active_until TIMESTAMPTZ NOT NULL DEFAULT now()+interval '10 minutes'
);

CREATE FUNCTION illya_pm_channel(first_user BIGINT,second_user BIGINT) RETURNS BIGINT LANGUAGE plpgsql AS $$
DECLARE channel BIGINT;
BEGIN
    INSERT INTO channels(name,kind,required_permission,dm_user_low,dm_user_high)
    VALUES('pm:'||LEAST(first_user,second_user)||':'||GREATEST(first_user,second_user),'PM','chat.pm',LEAST(first_user,second_user),GREATEST(first_user,second_user))
    ON CONFLICT(dm_user_low,dm_user_high) WHERE kind='PM' DO UPDATE SET kind='PM' RETURNING id INTO channel;
    RETURN channel;
END $$;

CREATE FUNCTION illya_bridge_private_message() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE channel BIGINT;
BEGIN
    channel:=illya_pm_channel(NEW.sender_id,NEW.recipient_id);
    INSERT INTO lazer_channel_members(user_id,channel_id) VALUES(NEW.sender_id,channel),(NEW.recipient_id,channel) ON CONFLICT DO NOTHING;
    INSERT INTO chat_messages(channel_id,user_id,content,created_at,is_action,client_uuid,private_message_id)
    VALUES(channel,NEW.sender_id,NEW.content,NEW.created_at,NEW.is_action,NEW.client_uuid,NEW.id);
    RETURN NEW;
END $$;
CREATE TRIGGER protocol_private_chat_bridge AFTER INSERT ON protocol_private_messages FOR EACH ROW EXECUTE FUNCTION illya_bridge_private_message();

-- Keep existing stable conversations available without exposing them as public channels.
INSERT INTO channels(name,kind,required_permission,dm_user_low,dm_user_high)
SELECT DISTINCT 'pm:'||LEAST(sender_id,recipient_id)||':'||GREATEST(sender_id,recipient_id),'PM','chat.pm',LEAST(sender_id,recipient_id),GREATEST(sender_id,recipient_id)
FROM protocol_private_messages WHERE sender_id<>recipient_id;
INSERT INTO lazer_channel_members(user_id,channel_id)
SELECT dm_user_low,id FROM channels WHERE kind='PM' UNION SELECT dm_user_high,id FROM channels WHERE kind='PM' ON CONFLICT DO NOTHING;
INSERT INTO chat_messages(channel_id,user_id,content,created_at,is_action,client_uuid,private_message_id)
SELECT c.id,m.sender_id,m.content,m.created_at,m.is_action,m.client_uuid,m.id FROM protocol_private_messages m JOIN channels c ON c.kind='PM' AND c.dm_user_low=LEAST(m.sender_id,m.recipient_id) AND c.dm_user_high=GREATEST(m.sender_id,m.recipient_id);

CREATE FUNCTION illya_chat_message_event() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('illya_chat',json_build_object('schema',current_schema(),'kind','message','id',NEW.id,'channel',NEW.channel_id)::text);
    RETURN NEW;
END $$;
CREATE TRIGGER native_chat_message_event AFTER INSERT ON chat_messages FOR EACH ROW EXECUTE FUNCTION illya_chat_message_event();
CREATE FUNCTION illya_chat_membership_event() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        PERFORM pg_notify('illya_chat',json_build_object('schema',current_schema(),'kind','part','user',OLD.user_id,'channel',OLD.channel_id)::text);
        RETURN OLD;
    END IF;
    PERFORM pg_notify('illya_chat',json_build_object('schema',current_schema(),'kind','join','user',NEW.user_id,'channel',NEW.channel_id)::text);
    RETURN NEW;
END $$;
CREATE TRIGGER native_chat_membership_event AFTER INSERT OR DELETE ON lazer_channel_members FOR EACH ROW EXECUTE FUNCTION illya_chat_membership_event();
