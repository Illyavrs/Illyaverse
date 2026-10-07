# Website chat

The `/chat` page reads public channels and authenticated private conversations. It uses the same `chat_messages`, PM channel pairs and private-message bridge as Resonance. Website PM sends enter `protocol_private_messages`; the existing transaction trigger mirrors them into website/native history and stable delivery.

The sidebar provides public channels, private conversation previews and unread counts, an inline username field to open a conversation, and a friend-only message preference. Desktop retains the existing navigation/transcript composition; mobile reflows the same controls. Labels and connection/error states are available in zh-TW, English and Japanese. Guest readers can access public channels and sign in to send messages or open PMs.

## HTTP contract

| Route | Behavior |
| --- | --- |
| `GET /api/v1/conversations` | Paginated joined PMs visible to the authenticated participant; includes peer, visible preview, unread count, read marker and send policy. |
| `POST /api/v1/conversations` | Strict JSON with one `target_username` or `target_id`; create/rejoin the unique pair. |
| `GET /api/v1/conversations/{id}` | Participant-authorized conversation metadata. |
| `GET /api/v1/conversations/{id}/messages` | Chronological history with page/page_size, exclusive before_id or after_id; maximum 100 items. |
| `POST /api/v1/conversations/{id}/messages` | Strict content, optional is_action and required client_uuid; return the persisted mirrored message. |
| `PUT /api/v1/conversations/{id}/read` | Strict message_id; acknowledge an actual visible message in this conversation, monotonically. Returns 204. |
| `POST /api/v1/chat/visibility` | At most 256 positive message IDs; return the currently readable subset. Guests receive public visibility only. Supplied invalid/expired website credentials remain unauthorized. |
| `GET /api/v1/chat/stream` | Authenticated cookie WebSocket for authorized public and PM events. |

Conversation metadata has `id`, `peer`, `last_message`, `last_read_id`, `unread_count`, `can_send` and `send_restriction`. The ID is the persisted message channel ID. Message DTOs include their authenticated sender, ID, channel, content, creation time, `is_action` and nullable `client_uuid`.

Public-channel sends retain the existing content-only API and also accept action/UUID fields. Public lists, history, sends and channel streams continue to exclude PM and native private-room sentinels, including for wildcard administrators.

## Authorization and retries

Every PM endpoint requires the actual low/high participant identity. A membership row or administrative permission does not grant access to another pair. Peer restriction or a block in either direction hides the conversation. Visible history, previews and counts use the same sender/relationship filters.

Creation and new sends require `chat.send`, a current website session and recipient policy. Friend-only means the recipient has added the sender as a friend; the active stable friend-only setting also applies. Existing otherwise-readable history stays readable when this preference changes. The typed `pm_friends_only` account field defaults to false and survives partial appearance/locale updates. The website checkbox PATCHes only this field.

Mutations use ordered user locks and current session/permission fences. Recipient policy is rechecked after resource waits, and actor expiry/revocation is checked before commit. Bodies are limited to 8 KiB, messages to 1,000 Unicode characters and client identifiers to 128 permitted ASCII characters. Create/send rates are limited per account as well as by the existing HTTP guard.

UUID uniqueness is per sender across public channels and PMs. The same channel, trimmed content and action with the same UUID returns the original message. Changed content/action/channel returns 409. Restriction, participant and block checks precede reuse; friend-only applies to a new send. The browser retains the UUID for an ambiguous send retry, deduplicates live echoes by persisted ID and sender-scoped UUID, and does not turn an accepted POST's refresh failure into a failed send.

## Live delivery and recovery

One lazily initialized PostgreSQL listener per Sanctuary state filters commit notifications by database schema. A subscriber is established before the socket emits `ready`. Events contain `type: messages` and an `items` array; `refresh` requests authoritative reconciliation periodically and after membership events. The existing `/channels/{id}/stream` uses the same commit-event runtime. It no longer relies on a maximum message ID, so a lower ID committed later can still arrive.

Delivery checks the current session, channel permissions, PM participant and sender/peer visibility. Sessions are rechecked every second. Sockets are bounded to 512 per process and four per user, with a 256-event receiver, 2 KiB incoming frames, 64 KiB emitted JSON and two-second writes. Lost listener connections or receiver lag close the socket for REST recovery.

The browser reconnects with bounded delays, refetches history and conversation metadata, and validates cached message IDs in bounded batches, including previously loaded older pages and sidebar previews. A policy denial removes private cached content. Signing out or changing account remounts chat state and clears its private histories/drafts. Session refresh also updates the public composer after permission changes. Guest public readers use polling.

Read markers are shared with native chat. The existing numeric marker treats a lower ID committed after a higher acknowledged ID as already read; commit-event delivery repairs live message loss, but does not change that read-receipt model. Stable currently resolves DM recipients through active stable/recent lazer presence and does not replay offline PMs at login. A website-only recipient therefore still needs native presence for stable-initiated messaging; persisted history is available through the website/native API.

## Verification scope

`website-chat.spec.ts` uses intercepted synthetic API/socket fixtures for desktop/mobile, older history, late IDs, sender-scoped UUIDs, ambiguous retries, draft preservation, creation/preference errors, policy cache removal and reconnect/guest behavior. `website-chat-live.spec.ts` writes only an explicitly owned disposable Compose project and exercises real website routes/socket delivery, native HTTP history/action sends and stable binary PM polling/sends. Its synthetic stable recipient resolution also authenticates the actor through native OAuth to satisfy the current native-presence boundary.

Sanctuary's opt-in `website_conversations_commit_delivery_and_mutation_fences` test uses a separate random PostgreSQL schema for participant/admin/private-room exclusion, blocks/restrictions, retry/bridge/read-marker behavior, friend-only preferences, queued mutation authorization and late-commit WebSocket delivery. Rate-capacity tests check that expired entries can be pruned at the exact shared-map limit.

These fixtures do not launch a game or prove actual stable/lazer UI behavior. Human stable score submission remains a separate pending diagnostic retry. Full community feature completion and production load/reconnection behavior are not established by this chat checkpoint.
