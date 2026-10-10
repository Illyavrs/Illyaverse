---
title: "Lazer realtime hubs"
---

Resonance implements an original Rust subset of the public lazer hub contracts. The HTTP, WebSocket and SSE integration test passed on 2026-10-07 using an isolated PostgreSQL schema and an ephemeral TCP listener. This establishes the tested server behavior. A native lazer application has **not** been run against this deployment; gameplay, playback latency, TLS and reconnect timing still require client validation.

A separate offline probe loaded the installed `osu.Game` DLL identified as `2026.1005.1-tachyon+6cbcae7fbb764839a66c1adf564c2e1679242072`, MessagePack `3.1.8` and its actual `SignalRUnionWorkaroundResolver`. Six typed fixtures passed native serialization/deserialization and then native bytes → frozen Rust codec → native typed deserialization: APIMod/settings, MultiplayerRoom, StandardMatchRoomState, SpectatorState, FrameHeader/replay-frame bundle and DateTimeOffset. The frame bundle grew from 123 to 131 bytes because mouse coordinates were represented as float64 through the Rust JSON value adapter; typed deserialization preserved the tested values. This is serializer interoperability evidence for that installed DLL identity, not stock-client network or gameplay verification. No upstream source or client DLLs are distributed in this repository.

## Transport and authentication

The hubs are `/multiplayer`, `/spectator` and `/metadata`, with equivalent `/signalr/*` aliases. Each supports `POST /negotiate`, WebSocket upgrade, text SSE, HTTP POST for SSE input, and authenticated DELETE to close a connection. Negotiation advertises WebSockets with text/binary transfer formats and SSE with text. Long polling, streaming hub invocations and stateful reconnect acknowledgements are not advertised or implemented.

Both protocols begin with the standard JSON handshake terminated by byte `0x1e`. JSON records use that separator; MessagePack records use the SignalR variable-length binary prefix. Incoming and outgoing records are limited to 128 KiB, buffered input to 256 KiB, batch size to 64 records, and nested MessagePack values to depth 48. A connection has a bounded 64-message outgoing queue; an overloaded connection is closed. Negotiation expires after 30 seconds when no transport attaches, and handshakes time out after 10 seconds.

Authentication accepts the existing opaque lazer access token in a Bearer header, or the SignalR `access_token` query parameter. Connection tokens are random and bound to the authenticated user and hub. Current restrictions, token revocation and `chat.send` permission are checked on each invocation and periodically while connected. Browser Origin must equal `PUBLIC_ORIGIN`; native requests may omit Origin. Request logging uses the URI path rather than credential-bearing query strings.

## Implemented hub methods

| Hub | Supported methods | Authority and behavior |
| --- | --- | --- |
| Multiplayer lounge | `CreateRoom`, `JoinRoom`, `JoinRoomWithPassword` | Persistent head-to-head rooms, up to 16 members and 100 playlist items. Password checks, blocked-member checks, actual stored map checksum/mode/difficulty, private chat-channel membership and real REST room discovery. Public room JSON exposes `has_password` and never the password. |
| Multiplayer room | `LeaveRoom`, `TransferHost`, `KickUser`, `ChangeSettings` | Membership and host authority are enforced inside a transaction locking the room. Closed rooms remain as history. Host changes and kicks cannot target users outside that room. |
| Multiplayer gameplay | `ChangeState`, `ChangeBeatmapAvailability`, `ChangeUserStyle`, `ChangeUserMods`, `StartMatch`, `AbortMatch`, `AbortGameplay`, `VoteToSkipIntro` | Only ready players with locally available maps can start. Loading, ready-for-gameplay, playing and finished states follow an explicit server state machine. Gameplay begins after participants load; results are announced after all active players finish. Spectator states do not count as fabricated completed scores. |
| Multiplayer playlist and invitations | `AddPlaylistItem`, `EditPlaylistItem`, `RemovePlaylistItem`, `InvitePlayer` | Changes follow host/queue authority, immutable stored map checksums, playlist bounds and block rules. Invitations use authenticated sender identity. |
| Standard match requests | `SendMatchRequest` for lock state and slot change | Native union tags 5 and 7, bounded slots, persistent authoritative slot order, occupied-slot and locked-slot rejection. Team changes, ranked-play requests and countdown requests return an explicit unsupported error. |
| Spectator | `BeginPlaySessionV2`, `SendFrameDataV2`, `EndPlaySessionV2`, `StartWatchingUser`, `EndWatchingUser`, plus legacy V1 methods | Optional score tokens must belong to the playing user/map/ruleset. Watchers cannot watch themselves or blocked/restricted users. Frames and headers are bounded and normalized, with a server receipt timestamp. At most eight users can be watched per connection. |
| Metadata | `GetChangesSince`, `UpdateActivity`, `UpdateStatus`, `BeginWatchingUserPresence`, `EndWatchingUserPresence`, `RefreshFriends`, `BeginWatchingMultiplayerRoom`, `EndWatchingMultiplayerRoom` | Actual local official-map update records and current presence/friend relationships. Activity payloads require known bounded union shapes. Room watches require authorized live membership or history. Original room-score source supplies committed item statistics and `MultiplayerRoomScoreSet` events; each visible historical result requires the exact frozen round or explicit score/moderator authority. A later joiner receives empty prior statistics. Solo scores do not enter room distributions. |

Room state, private chat membership and token-bound replay capture chunks live in PostgreSQL. Live spectator fanout, transport queues and presence subscriptions are transient. Ended captures with committed score references recover through bounded startup/periodic export; reconnects retain chunks and disclose interrupted continuity. The extended replay contract and native DLL proof describe exact validation and privacy. A gateway restart closes interrupted lazer rooms and releases their memberships while preserving room history. Run one Resonance gateway instance; transient hubs are not coordinated across multiple processes. Stable and lazer rooms are separate and cannot currently be joined across client generations.

Auto-start durations and unsupported match types are rejected explicitly. Daily challenges, playlist-room creation, editing activity unions, referee/tournament interfaces, quick play, ranked play and deterministic replay verification remain outside this subset. Completion or spectator frames alone do not verify a solo score. Accepted native submissions publish automatically after their required replay checks, with a complete bound capture required for passed lazer scores; API submissions, incomplete captures and explicit administrator decisions retain staging/review. The latest human report confirms submissions working normally, not deterministic replay validation or native room/private-message behavior. See the current integration checkpoint.

Original source implements HeadToHead room-bound score submission, aggregation, metadata statistics/events and `UserScoreProcessed` delivery to the exact submitting ended connection. `ResultsReady` remains independent of score processing. Four fresh PostgreSQL and loopback JSON/MessagePack HTTP/WebSocket checks pass, covering durability, privacy, totals, races and queued authorization. A separate pinned native-DLL probe passes 19 DTO assertions; it consumes four actual HTTP responses and re-encodes the decoded server event tuples with the production codec. Pinned native gameplay and display checks still require a human client session. The room-score contract and verification checklist records durable identity, recovery and result privacy. Fixture coverage of playlist aggregation rules does not implement playlist-room creation.

## Native model serialization

The implementation was checked against the official client model attributes and custom formatter contracts, rather than assuming JSON objects can be emitted as MessagePack maps. Representative native payloads are exercised over real WebSockets by the integration test.

| Model | MessagePack representation |
| --- | --- |
| `MultiplayerRoom` | Nine-element keyed array: room ID, state, settings, users, host, match state, playlist, countdowns and channel ID. |
| `MultiplayerRoomSettings` | Eight-element array; `TimeSpan` is ticks, enums are integer values, and maximum participants is nullable. JSON uses a textual zero duration. |
| `MultiplayerRoomUser` | Nine-element array including availability, mods, nullable match/style state and role. |
| `MultiplayerPlaylistItem` | Twelve-element array, including authoritative map checksum/difficulty and nullable played-at time. |
| `MultiplayerPlaylistItemStats` | Four-element array: playlist item ID, 13-bin distribution, cumulative passing total and last processed score ID. Isolated room-score integration and pinned-DLL deserialization checks pass. |
| `MultiplayerRoomScoreSetEvent` | Six-element array: room ID, playlist item ID, score ID, user ID, native total and nullable new rank; one hub argument. `UserScoreProcessed` separately has two scalar arguments. |
| Standard match state | Union `[3, [null, locked, slots]]`. A non-null participant limit produces a slots array of that length. |
| `APIMod` | `[acronym, settings]`. The official custom settings formatter writes an **array pair count followed by key/value tokens**, which is not a normal MessagePack map. The bounded Rust reader/writer adapts this specific model shape; empty and non-empty settings are covered, including DT `speed_change`. |
| `SpectatorState` | Five-element array. Hit statistics use integer enum keys (`Great = 5`), retained through JSON adaptation. |
| `FrameDataBundle` / `FrameHeader` | Two-element bundle and ten-element header. Nested score-processor statistics use their five keyed fields. |
| `LegacyReplayFrame` | Four-element array: time, nullable X/Y and integer button flags. |
| `DateTimeOffset` | `[MessagePack timestamp extension, UTC offset minutes]`; server receipt timestamps use offset zero. |
| `UserPresence` / activity | Presence is `[activity, status]`; supported activities retain the official integer-tagged union envelope. |

The custom mod-settings shape is recognized only for the public two-element acronym/settings model. Unknown serializer contracts are rejected rather than treated as complete client support. The test covers typed array/key order and custom token layout using original fixtures; it does not load the official client assemblies.

## Reproducible verification

Run the default codec/model tests and lint checks from the monorepo root:

```powershell
cargo test -p resonance-protocol -p resonance
cargo clippy -p resonance-protocol -p resonance --all-targets -- -D warnings
```

The live test requires a disposable local PostgreSQL database. It creates and destroys its own random schema, uses synthetic users and a fixture map only within that schema, and aborts its ephemeral server before returning:

```powershell
$env:ILLYA_TEST_DATABASE_URL = 'postgres://<test-user>:<test-password>@127.0.0.1:<test-port>/<test-database>'
cargo test -p resonance signalr_http_websocket_and_sse -- --ignored --nocapture
```

The passing test covers unauthenticated negotiation, rejected browser Origin, connection-token ownership, private room password hiding, wrong-password and non-host rejection, ready/load/start/complete, host transfer/leave, actual native-format room creation, slot movement/locking, JSON and MessagePack metadata, non-empty native mod settings, cross-protocol spectator states/frames, native receipt timestamps, blocked spectators, revoked sessions, and SSE negotiation/handshake/invocation/deletion.

## Primary contracts consulted

- [SignalR hub wire specification](https://github.com/dotnet/aspnetcore/blob/main/src/SignalR/docs/specs/HubProtocol.md) and [transport specification](https://github.com/dotnet/aspnetcore/blob/main/src/SignalR/docs/specs/TransportProtocols.md).
- [Official lazer hub connector](https://github.com/ppy/osu/blob/master/osu.Game/Online/HubClientConnector.cs), which selects MessagePack, and its [union resolver](https://github.com/ppy/osu/blob/master/osu.Game/Online/SignalRUnionWorkaroundResolver.cs).
- [Multiplayer server interface](https://github.com/ppy/osu/blob/master/osu.Game/Online/Multiplayer/IMultiplayerRoomServer.cs), [client callbacks](https://github.com/ppy/osu/blob/master/osu.Game/Online/Multiplayer/IMultiplayerClient.cs), [room model](https://github.com/ppy/osu/blob/master/osu.Game/Online/Multiplayer/MultiplayerRoom.cs), [settings](https://github.com/ppy/osu/blob/master/osu.Game/Online/Multiplayer/MultiplayerRoomSettings.cs), [room user](https://github.com/ppy/osu/blob/master/osu.Game/Online/Multiplayer/MultiplayerRoomUser.cs) and [standard match state](https://github.com/ppy/osu/blob/master/osu.Game/Online/Multiplayer/StandardMatchRoomState.cs).
- [Official APIMod settings formatter](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/ModSettingsDictionaryFormatter.cs), [spectator state](https://github.com/ppy/osu/blob/master/osu.Game/Online/Spectator/SpectatorState.cs), [frame header](https://github.com/ppy/osu/blob/master/osu.Game/Online/Spectator/FrameHeader.cs), [legacy replay frame](https://github.com/ppy/osu/blob/master/osu.Game/Replays/Legacy/LegacyReplayFrame.cs) and [score processor statistics](https://github.com/ppy/osu/blob/master/osu.Game/Rulesets/Scoring/ScoreProcessor.cs).
- [MessagePack-CSharp DateTimeOffset formatter](https://github.com/MessagePack-CSharp/MessagePack-CSharp/blob/master/src/MessagePack/Formatters/StandardClassLibraryFormatter.cs).

These sources establish contracts. Their server or client implementations were not incorporated into Illyaverse.
