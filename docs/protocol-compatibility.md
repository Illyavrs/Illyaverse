# osu! client compatibility

Resonance is an original Rust implementation of the protocol boundaries. Reference projects were read to understand wire formats and API contracts; their server code was not incorporated. Compatibility is experimental until the release checklist has been exercised with actual client builds.

## Current coverage

| Area | Implemented behavior | Remaining validation or limitation |
| --- | --- | --- |
| stable login | Three-line login, MD5 password verified against Argon2id, opaque `cho-token`, token hashes stored in PostgreSQL, 180-second polling session | Actual client login and TLS/host switching still require validation |
| stable Bancho | Bounded binary framing, ping, status, presence, channel join/part, public chat, direct messages and friend-only DM settings | Actual native client packet timing remains to be validated |
| stable multiplayer | Durable 16-slot rooms, Argon2-protected passwords, authenticated host/slot authority, ready/start/load/score/complete, host transfer, room chat and leave/expiry cleanup | Tournament interfaces, advanced team result calculation and cross-client rooms remain unsupported |
| stable spectating | Authenticated watch/stop, peer notifications, bounded finite replay-frame bundles and spectator chat | Actual native replay playback/latency and disconnect timing require validation |
| Chat permissions and delivery | Authenticated native notification WebSocket, `chat.start`/`chat.end`, join/part/message events with sender models, native PM creation, history/read markers, UUID retry deduplication, keepalive and shared website/stable messages | Actual lazer UI behavior and reconnect timing remain unverified; announcement channels and non-chat notification types remain unsupported. See [native chat contracts](native-chat.md) |
| stable score submission | Multipart encrypted score envelope, Rijndael with 256-bit blocks and CBC, strict padding, checksum, login/client hash binding, authenticated map identity, idempotency | Structural integrity is not a deterministic gameplay or anti-cheat verdict |
| PP | Sanctuary calculates performance using rosu-pp from the stored `.osu`, complete judgments/ticks, combo, ruleset and validated structured GameMods; supported speed, DA/CL/HD and ruleset settings persist as `api_mods`; standard maps can convert to taiko/catch/mania | Unknown, duplicate, incompatible, out-of-range and unsupported settings are rejected. See [native mod policy](native-mods.md) |
| Rankings | Pending submissions stay out of rankings; moderators can review and mark scores verified; `pp_eligible` excludes DA/RX/AP from ranked PP and native mod filters compare validated acronym sets | Moderation is a human review policy, not automatic cheat detection |
| Replays | Stable raw LZMA frames are validated and exported as `.osr`; lazer spectator captures persist bounded chunks, bind to score metadata, recover pending exports and support native binary downloads plus owned extended `.osr` uploads | Deterministic replay simulation and actual game-host playback remain unverified; interrupted captures are disclosed and stay pending. See [lazer replay contracts and native DLL evidence](lazer-replays.md) |
| Local maps | `.osu` retrieval, uploaded `.osz` download, osu!direct search, imported official IDs, beatmap hash matching | Filename-based unknown-map lookup is incomplete; an uploaded `.osu` alone has no `.osz` archive |
| Official maps | Sanctuary performs OAuth-backed official imports with server-side throttling and retry handling | Requires the operator's own osu! OAuth credentials and access to the official service |
| lazer OAuth | Password grant, refresh grant, separate opaque access/refresh tokens; refresh rotation invalidates both old tokens; user restrictions rechecked | Client identifiers are treated as public native-app identifiers; no official client secret is embedded |
| lazer solo scores | Native score-token route and JSON submission, authenticated user/map/ruleset binding, retry deduplication, persistent submission statistics | Accuracy is labeled client-reported; PP uses complete supplied judgments/ticks |
| lazer API | Current user/profile with verified database statistics, checksum lookup, official/local ID resolution, beatmapset lookup/search/archive download, score collections/details/download, public chat REST and room membership checks | Full official API parity and filename-only lookup are not yet available; untracked play time and replay-watch metrics are omitted |
| Friends and blocks | Durable directed relationships, stable friend packets, lazer relation API, blocked-player DM enforcement and shared website controls | Each mutation requires the current `social.manage` permission |
| lazer realtime | SignalR JSON/MessagePack codecs, authenticated WebSocket/SSE routes and room/spectator/metadata subsets | Real transport tests are separate from the native client release checklist; a single gateway owns transient streams. See [detailed hub contracts](signalr-compatibility.md) |

Unimplemented endpoints must not be advertised as completed features. A successful synthetic request proves the tested server contract, not full native-client compatibility.

## Deployment contract

Resonance listens on `RESONANCE_BIND` (default `0.0.0.0:8081`) and shares PostgreSQL migrations with Sanctuary. Set `DATABASE_URL`, `STORAGE_PATH`, `SANCTUARY_INTERNAL_URL`, and an `INTERNAL_SERVICE_KEY` of at least 32 bytes. Sanctuary and Resonance must mount the same maps/replays storage. Only hashed protocol tokens are persisted; the website session cookie uses a separate session format.

The reverse proxy must route Bancho hosts and `/web/*`, `/osu/*`, `/d/*`, `/oauth/*`, `/api/v2/*`, `/signalr/*` and `/notification-server` to Resonance. The spectator host routes `/multiplayer`, `/spectator` and `/metadata`, including WebSocket upgrades. The native chat URL comes from `LAZER_NOTIFICATION_ENDPOINT`, or the configured public origin with the WebSocket scheme and `/notification-server` path; it cannot contain credentials, query parameters or fragments. Set `ILLYA_DOMAIN` for the restricted `a.<domain>` avatar endpoint. Forwarded IPs are trusted only when the immediate socket peer is in `TRUSTED_PROXY_CIDRS`. Query strings containing stable credentials are excluded from gateway request logs. Public score JSON never includes password hashes or protocol token hashes.

Run one Resonance gateway instance for the current realtime implementation. PostgreSQL retains rooms, private-channel memberships and score-token replay capture chunks; live spectator fanout and transport queues use memory. On restart, interrupted lazer rooms close and memberships are released; room history remains available. Ended replay captures already bound to scores recover in bounded startup/periodic batches.

Switching DNS or hosts entries only selects a server address. TLS trust, the correct protocol hostnames, API response compatibility, and native realtime transports must also work. Use the reversible Kaleidoscope connection helper and the [connection guide](connection.md); do not disable certificate verification to hide a deployment error.

## Validation evidence

On 2026-10-07, the independent live HTTP smoke test passed against a disposable PostgreSQL 18 instance, Sanctuary at `127.0.0.1:18080`, and Resonance at `127.0.0.1:18081`. It exercised stable binary login/ping/chat/logout, sender spoof prevention, shared authentication, lazer OAuth refresh revocation, staff-channel RBAC, solo-token identity binding, encrypted stable submission, checksum rejection, altered-replay rejection, server PP calculation, replay round trip, retry deduplication, and pending-to-reviewed ranking behavior.

The expanded live test also passed directed friends/blocks, friend-only DMs, password-protected stable rooms, non-host start rejection, rewritten authoritative score slots, ready/start/load/score/complete/host-transfer/leave transitions, and stable spectator frame relay. Native API checksum/set lookup, archive download and map score collections passed. After review, the fixture appeared in native rankings and its profile returned nonzero weighted PP, ranked score, global rank, combo and a level derived from verified total score.

The final gateway/protocol check passed 23 codec/model tests and 11 gateway unit tests, plus Clippy with warnings denied across all targets. These cover whole-batch packet rejection, every truncated packet/container prefix, secret redaction, bounded fields, password hiding, malformed realtime frames, level thresholds and avatar boundaries.

The separate `signalr_http_websocket_and_sse` integration test passed against an isolated PostgreSQL schema and a real ephemeral TCP listener. It exercised JSON and native-format MessagePack WebSockets, SSE negotiation/handshake/input/delete, room authority and gameplay transitions, typed spectator frames and timestamps, metadata, blocks and token revocation. It uses original synthetic fixtures, not an actual lazer executable or its loaded C# serializers. [Detailed evidence and the reproducible command](signalr-compatibility.md#reproducible-verification) record its exact scope.

The `native_chat_http_and_websocket` test passed against a separate random PostgreSQL 17 schema and a real ephemeral HTTP/WebSocket listener. It exercised the native notification URL and event shapes, sender/UUID echoes, deferred commits, concurrent retries and opposite PM sends, friend-only/block rules, channel history/read markers, keepalive expiry, private-room and channel membership revocation, stable/native binary chat interoperability, repeated start/end/resume and session expiry/revocation/restriction. Four chat unit tests also passed. [Native chat evidence](native-chat.md#verification) records the command and limits; no native game application was launched by these tests.

The score cipher fixture is independently encrypted by the test-only Python `py3rijndael` library. The Rust implementation uses `simple-rijndael` for the block primitive and original framing/CBC/validation code. This prevents the test from merely encrypting and decrypting with the same implementation.

Run codec/model tests:

```powershell
cargo test -p resonance-protocol -p resonance
cargo clippy -p resonance-protocol -p resonance --all-targets -- -D warnings
```

Run the live test only against an explicitly disposable local environment. It uploads a fixture map, creates scores and moderates its fixture score:

```powershell
python -m venv .protocol-venv
.protocol-venv/Scripts/python -m pip install -r services/resonance/tests/requirements.txt
$env:ILLYA_ALLOW_DISPOSABLE_TEST = '1'
$env:TEST_ADMIN_USERNAME = '<test administrator>'
$env:TEST_ADMIN_PASSWORD = '<test password>'
$env:TEST_PLAYER_USERNAME = '<test player>'
$env:TEST_PLAYER_PASSWORD = '<test password>'
$env:TEST_SANCTUARY_URL = 'http://127.0.0.1:18080'
$env:TEST_RESONANCE_URL = 'http://127.0.0.1:18081'
.protocol-venv/Scripts/python services/resonance/tests/protocol_smoke.py
```

The script rejects remote hosts and requires an explicit disposable-test flag. Never run it against a live community database. No fixture accounts or maps are inserted by production migrations.

## Native client release checklist

Record exact stable and lazer build versions when exercising these flows:

- Login, logout, reconnect, expired credentials and restricted accounts.
- Public chat, direct messages, friends/blocks, permission revocation and staff visibility.
- Download/import an official map and a private uploaded map; check osu!direct archive handling.
- Play each ruleset, submit passed/failed scores, retry after interruption, download/watch the saved replay.
- Validate supported mods and reject unsupported settings without producing misleading PP.
- Confirm pending scores remain private to the owner/moderators and appear in rankings only after review.
- Validate native realtime transport negotiation, rooms, spectators, host transfer, ready/load/complete and disconnect recovery.
- Check TLS trust and reversible host changes, including restoration after the helper is removed.

Specific remaining features include stable Target Practice's extra score/replay field, in-editor map publishing, tournament interfaces, unknown filename resolution, unsupported lazer mod settings, announcement channels and non-chat notification types. Stable and lazer currently use separate multiplayer room models. Native map score leaderboards compare reported lazer standardized totals with passed, convertible stable estimates; unsupported score scales are excluded. Stable map boards preserve their raw-score source. Shared profile totals use Classic display caches for native/ScoreV2 and raw stable V1 history; ranked totals select a standardized best result per map before adding its Classic value. PP selection remains independent. The original versioned converter is contract- and integration-tested, with actual gameplay still pending; see [score normalization](score-normalization.md).

The realtime subset also excludes long polling, stateful reconnect, quick/ranked play, unsupported match types and countdown requests, per-room solo score aggregation and `UserScoreProcessed` notifications. These limits are enumerated in the [hub compatibility document](signalr-compatibility.md). No native stable or lazer build has completed the release checklist above.

## Interoperability sources

The legacy packet framing and field ordering were checked against [bancho.py packet definitions](https://github.com/osuAkatsuki/bancho.py/blob/master/app/packets.py). Login and legacy HTTP contracts were checked against its [Bancho endpoint](https://github.com/osuAkatsuki/bancho.py/blob/master/app/api/domains/cho.py), [legacy web endpoints](https://github.com/osuAkatsuki/bancho.py/blob/master/app/api/domains/osu.py), and [encryption boundary](https://github.com/osuAkatsuki/bancho.py/blob/master/app/encryption.py).

The lazer login contract was checked against the official client's [OAuth implementation](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/OAuth.cs). Solo score routes and payloads were checked against [CreateSoloScoreRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/Solo/CreateSoloScoreRequest.cs), [SubmitSoloScoreRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/Solo/SubmitSoloScoreRequest.cs), and [SoloScoreInfo](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/Responses/SoloScoreInfo.cs). Friend response fields follow the official [APIRelation model](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/Responses/APIRelation.cs).

Replay container facts were checked against the official [.osr file format](https://osu.ppy.sh/wiki/en/Client/File_formats/osr_(file_format)) and the [official legacy replay encoder](https://github.com/ppy/osu/blob/master/osu.Game/Scoring/Legacy/LegacyScoreEncoder.cs). Separate API, website and realtime endpoints are explicit in the official client's [EndpointConfiguration](https://github.com/ppy/osu/blob/master/osu.Game/Online/EndpointConfiguration.cs).

Profile level thresholds follow the official [total score and level description](https://osu.ppy.sh/wiki/en/Gameplay/Score/Total_score). The profile uses verified persisted score totals and weighted best-map PP rather than placeholder statistics; moderation remains the ranking policy.
