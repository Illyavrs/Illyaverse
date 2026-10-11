-- A user group for bot accounts such as IllyaBot. It outranks every staff role except super_user,
-- so bots cannot be edited or reassigned by ordinary staff.
INSERT INTO roles(name, description, permissions, rank, short_name, colour, display_badge, icon, description_i18n)
VALUES ('bot', 'Automated server accounts such as IllyaBot.', ARRAY[]::text[], 900, 'BOT', '#8b9cff', true, 'bot',
        jsonb_build_object(
            'zh-TW', '伺服器的自動化帳號，例如 IllyaBot。它們不能登入，也不會出現在排行榜。',
            'en', 'Automated server accounts such as IllyaBot. They cannot sign in and do not appear in rankings.',
            'ja', 'IllyaBot などのサーバーの自動アカウントです。ログインできず、ランキングにも表示されません。'));
INSERT INTO user_roles(user_id, role_id) SELECT 1000000, id FROM roles WHERE name = 'bot';
