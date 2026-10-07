-- Website relationship management is independently assignable in RBAC.
UPDATE roles SET permissions = array_append(permissions, 'social.manage')
WHERE name IN ('player', 'moderator') AND NOT ('social.manage' = ANY(permissions));
