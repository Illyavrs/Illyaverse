# Website write authorization

Sanctuary checks authenticated writes inside their database transaction. An earlier HTTP authentication result is a snapshot: another request can revoke its session, restrict its account or remove its permission while upload processing, official-map discovery or a database lock is pending.

`auth::mutation_actor` takes the relevant permission fence, locks the actor and additional affected users in ascending ID order, locks the original web session unconditionally, and then checks the current database clock and account restriction. Permission-sensitive callers reload effective role permissions under that fence. Callers acquire resource rows afterward. `auth::ensure_mutation_actor` checks the captured session deadline after resource waits and immediately before publication or commit.

| Scope | Use | Permission fence |
|---|---|---|
| `Session` | Own preferences, profile, media, password and email-verification request | None; no role permission is needed |
| `Permissions` | Chat, relationships, map upload/import, website score/replay submission | Shared role/membership table fence |
| `RoleChanges` | Administration, moderation and RX recalculation | Serialized role/membership mutation fence |

The final check uses the original authorized session deadline. It permits an administrator to revoke their own sessions or demote themselves in that same transaction. Other requests cannot change the held user, session or permission records while the mutation is publishing. Natural expiry can still advance and causes rollback. The check occurs before COMMIT; it is not a claim about the wall-clock instant when the database finishes acknowledging COMMIT.

Password verification/hashing, multipart decoding, image conversion, map downloads and PP calculation happen before the short publication transaction. A password change compares the verified credential with the locked current credential before writing. Email issuance preserves recipient-limit row → user/session ordering, including a rate-limited request, so public recovery and authenticated verification do not introduce opposite lock orders. Public one-time-token confirmation retains its separate locked token authorization.

Chat also locks the channel policy before inserting a message. Restricted native room/PM channels stay unavailable to website posting. Relationship changes retain the same ascending actor/peer order used by native transports; relationship rows and generated Bancho notifications commit together.

Manual official-map discovery carries the website actor into the cold map-insertion transaction. Warm and coalesced results recheck authorization before their import audit. Unauthorized failures are not cached as unknown maps. Trusted internal score ingress uses a fresh active-user row lock at publication; its service key remains a separate authority from a web session.

Media staging resides in a private directory inside the configured media filesystem. Only avatar, banner and background directories have static routes, so staged bytes are not served through direct, encoded or traversal paths. Owned staged/published files are removed on definite rejection. A lost COMMIT acknowledgement can follow a successful database commit; files are retained when that outcome is uncertain. Map uploads and replay publication follow the same definite-failure cleanup rule.

The ignored PostgreSQL 17 fixtures create independent random schemas and synthetic assets. `auth::mutation_tests`, `chat::mutation_tests`, `maps::map_mutation_tests` and `scores::score_mutation_tests` use actual lock barriers and database deadlines to check revocation, restriction, permissions, late expiry, self-revocation/demotion, channel-policy changes, native packet rollback and file cleanup. They do not operate a game client or prove native score submission. Runtime verification is recorded in [verification](../apps/docs/content/docs/reference/verification.md).
