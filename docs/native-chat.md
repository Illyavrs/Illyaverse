# Native lazer chat

Resonance implements original HTTP and notification-WebSocket chat contracts. The isolated real TCP integration test and a dedicated persisted-reconnect fixture passed on 2026-10-09. They use synthetic requests following public native models; they do not launch lazer or establish native UI, client reconnection or playback compatibility.

## Connection and events

`GET /api/v2/notifications` authenticates the opaque lazer access token and returns `notification_endpoint`. General notifications are currently an empty collection; achievement, forum and other notification delivery is not implemented. The endpoint is configured by `LAZER_NOTIFICATION_ENDPOINT`, or derived from the deployment's public origin as `ws`/`wss` plus `/notification-server`. Credentials, query strings and fragments are rejected.

The native client upgrades that endpoint with `Authorization: Bearer <access token>`, then sends JSON `chat.start`. The first start on each fresh socket sends authorized joined-channel events before replaying a bounded recent persisted-message window. Repeated starts and `chat.end`/`chat.start` on the same socket do not replay that window again. `chat.end` pauses live delivery. There is no SignalR handshake on this socket. The upstream transport follows the official [connector](https://github.com/ppy/osu/blob/master/osu.Game/Online/Notifications/WebSocket/WebSocketNotificationsClientConnector.cs), [notification response model](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/Responses/APINotificationsBundle.cs) and [WebSocket command contract](https://github.com/ppy/osu-web/blob/master/resources/views/docs/_websocket_commands.md).

The server sends `chat.channel.join`, `chat.channel.part` and `chat.message.new`. A message event contains both `messages` and corresponding `users`, including the authenticated sender ID. Messages include their persisted ID, channel, timestamp, action flag and original client UUID. This matches the official [chat client](https://github.com/ppy/osu/blob/master/osu.Game/Online/Chat/WebSocketChatClient.cs), [message event model](https://github.com/ppy/osu/blob/master/osu.Game/Online/Notifications/WebSocket/Events/NewChatMessageData.cs) and [event specification](https://github.com/ppy/osu-web/blob/master/resources/views/docs/_websocket_events.md).

## HTTP routes

| Route | Behavior |
| --- | --- |
| `GET /api/v2/chat/channels` | Joinable public channels, filtered by current account permissions. |
| `GET /api/v2/chat/updates?since=0&includes[]=presence` | Joined channel presence; repeated includes are supported. Deprecated messages and silence arrays remain empty. |
| `GET /api/v2/chat/channels/{id}` | Authorized channel details and PM participant models. |
| `PUT/DELETE /api/v2/chat/channels/{id}/users/{user}` | Join/leave using the authenticated owner's ID. Private room membership is checked separately. |
| `GET /api/v2/chat/channels/{id}/messages` | Bounded recent history with `since`, exclusive `until` and `limit` up to 50; restricted/blocked senders are excluded. |
| `POST /api/v2/chat/channels/{id}/messages` | Native form fields `message`, `is_action` and optional `uuid`. Identity comes from authentication. |
| `POST /api/v2/chat/channels` | Create/rejoin one-to-one `PM` using `target_id`; return non-null `recent_messages` containing up to 50 authorized messages. Other channel types return an unsupported response. |
| `POST /api/v2/chat/new` | Create/rejoin PM and send the initial message, returning channel and message. |
| `PUT /api/v2/chat/channels/{id}/mark-as-read/{message_id}` | Persist a monotonic read marker for an actual message in that channel. |
| `POST /api/v2/chat/ack` | Extend public chat activity by ten minutes; negative cursors are rejected. |

Public notification delivery pauses when chat activity expires. PM delivery continues independently of this timeout. This follows the upstream [chat usage contract](https://github.com/ppy/osu-web/blob/master/resources/views/docs/_using_chat.md). Native form and route details were checked against [CreateChannelRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/CreateChannelRequest.cs), [PostMessageRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/PostMessageRequest.cs), [MarkChannelAsReadRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/MarkChannelAsReadRequest.cs), [GetUpdatesRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/GetUpdatesRequest.cs) and [ChatAckRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/ChatAckRequest.cs).

Message, create-channel, initial-PM and acknowledgement bodies accept native scalar multipart, URL-encoded forms and JSON through the shared bounded native-form parser. Duplicate names, uploaded files, invalid UTF-8 and unknown chat fields are rejected. Native `is_action` is lowercase `true`/`false`. The pinned `2026.1005.1-tachyon` client sends initial chat acknowledgement as an empty POST without a Content-Type header; only the acknowledgement handler accepts that exact empty framing. A headerless non-empty body is rejected. Native `APIChatChannel.RecentMessages` is supplied on PM creation/rejoin because the client's channel manager consumes that collection directly.

## Persistence and authorization

Website and stable public writes share `chat_messages`. Stable private writes are bridged transactionally into a unique PM conversation; native PM writes persist through the same private-message table for stable delivery. Action messages translate to/from stable's action text envelope. No mirrored message is sent before its database transaction commits.

A PostgreSQL commit notification carries only IDs. Before transmitting its payload, Resonance fetches the persisted message and rechecks the access token, restrictions, membership, channel permissions and either-direction block rules. Private multiplayer channels require actual room membership, including for administrators. PM channels require a participant identity and stay excluded from website and stable public channel lists, even with wildcard administration permissions.

PM initiation and sends honor the recipient's friend-only preference, including the active stable setting. Ordered locks on both users serialize concurrent opposite sends and relationship changes. Client UUID reuse returns the existing message when the channel, content and action flag agree; conflicting reuse returns HTTP 409. Read markers cannot move backwards or claim a message in another channel.

Sanctuary website public history excludes restricted senders for everyone. Signed-in readers also exclude either-direction blocked senders, including from the visible-message count. The [website chat](website-chat.md) implementation adds participant-only PM routes and UI, commit-driven public/PM WebSocket delivery, bounded cached-history visibility reconciliation and typed friend-only preferences. The existing channel stream also uses commit notifications, including lower IDs committed after newer messages. Website public routes still exclude PM and private room channels, including for wildcard administrators. Native presence, read-marker and real-client verification boundaries are recorded separately in that document.

## Bounds and recovery

REST chat bodies are limited to 8 KiB, messages to 1,000 characters and client identifiers to 128 permitted ASCII characters. Socket commands are limited to 2 KiB and 60 per minute. The process admits at most 512 sockets and four per user. Each database-event receiver has a 256-event buffer, emitted JSON is limited to 64 KiB and socket writes time out after two seconds.

Authorization is rechecked on every command/event and periodically while connected. Token expiry/revocation or account restriction emits logout and closes the connection. Permission and membership changes remove inaccessible channel presence. A slow receiver or lost database notification connection closes affected sockets.

On a new socket's first `chat.start`, recovery examines at most the newest 50 raw persisted rows per currently authorized joined channel, then returns at most 200 visible messages across the socket in ascending persisted-ID order. Restriction and block filtering happens after the raw per-channel bound, so hidden recent rows do not trigger an unlimited search through older history. Each delivery rechecks the current token/account, access, blocks and membership. Public chat still requires unexpired acknowledgement activity; PM recovery requires an authorized participant and current conversation membership. Queued live notifications for already-recovered IDs are suppressed on that socket. The pinned [WebSocketChatClient](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Chat/WebSocketChatClient.cs) requests chat start and REST presence on reconnect without supplying a message cursor; [ChannelManager](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Chat/ChannelManager.cs) retains loaded channels. Recovery therefore preserves original IDs/UUIDs for client deduplication and leaves presence-only REST updates unchanged. Gaps older than this recovery window can be retrieved through bounded REST history. This is bounded recovery and live delivery, not an exactly-once notification archive.

The current implementation omits announcement/group channels, independent silence-history management and non-chat notification types. Full client UI, transport interruption and idle reconnection behavior still require native validation. The general SignalR room/spectator runtime remains a separate [hub implementation](signalr-compatibility.md).

## Verification

Five unit tests cover native command/form contracts, multipart/JSON/form parsing, empty initial acknowledgement, strict fields, cursor/message/identifier bounds, endpoint credential rejection and connection permit release. The opt-in real HTTP/WebSocket test creates and destroys its own random schema and ephemeral server:

```powershell
$env:ILLYA_TEST_DATABASE_URL = 'postgres://<test-user>:<test-password>@127.0.0.1:<test-port>/<test-database>'
cargo test -p resonance native_chat_http_and_websocket -- --ignored --nocapture
cargo test -p resonance persisted_reconnect_is_bounded_authorized_and_once_per_socket -- --ignored --nocapture
```

It passed notification URL/event/sender/UUID checks; multipart message/create/PM/ack requests and initial headerless empty acknowledgement; non-null recent PM history; rejection of duplicate fields, file parts and oversized bodies before writes; a lower message ID committed after a newer message; concurrent identical retries and conflicting reuse; opposite PM sends; friend-only and blocked history/new messages; public keepalive expiry with continuing PM delivery; private-room observers and revoked membership; channel leave before a subsequent write; native-to-stable action PMs and stable-to-native authenticated binary PMs; repeated start/end/resume; write-permission revocation; expired/deleted tokens and restricted accounts. No existing community schema, native client configuration, hosts file or certificate store is modified by this test.

The separate reconnect fixture exercises a real disconnected socket, persisted public/PM writes while offline, joins before replay, unchanged message and sender IDs, repeated start and end/start, duplicate queued commit notifications, subsequent live delivery, unauthorized PM and private-room membership including a wildcard administrator, blocks/restricted senders, left channels/conversations, both recovery bounds, hidden recent tails, and token/account revocation between upgrade and first start. It uses a disposable random schema and loopback HTTP/WebSocket server. Native client UI reconnection remains pending.
