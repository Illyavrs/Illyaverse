# osu! client compatibility

Resonance is an original Rust implementation of the protocol boundaries. Reference projects were read to understand wire formats and API contracts; their server code was not incorporated. Compatibility is experimental until the release checklist has been exercised with actual client builds.

## Current coverage

| Area | Implemented behavior | Remaining validation or limitation |
| --- | --- | --- |
| stable login | Three-line login, MD5 password verified against Argon2id, opaque `cho-token`, token hashes stored in PostgreSQL, 180-second polling session | Actual client login and TLS/host switching still require validation |
| stable Bancho | Bounded binary framing, ping, status, presence, channel join/part, public chat, direct messages and friend-only DM settings | Actual native client packet timing remains to be validated |
| stable multiplayer | Durable 16-slot rooms, Argon2-protected passwords, authenticated host/slot authority, ready/start/load/score/complete, host transfer, room chat and leave/expiry cleanup | Tournament interfaces, advanced team result calculation and cross-client rooms remain unsupported |
| stable spectating | Authenticated watch/stop, peer notifications, bounded finite replay-frame bundles and spectator chat | Actual native replay playback/latency and disconnect timing require validation |
| Chat permissions | Sender identity comes from authentication, channel permissions checked on each poll/request, shared persistent public chat with the website | Native lazer notification WebSocket delivery is not yet implemented |
| stable score submission | Multipart encrypted score envelope, Rijndael with 256-bit blocks and CBC, strict padding, checksum, login/client hash binding, authenticated map identity, idempotency | Structural integrity is not a deterministic gameplay or anti-cheat verdict |
| PP | Sanctuary calculates performance using rosu-pp from the stored `.osu`, judgments, combo, mode, mods and lazer tick counts; standard maps can convert to taiko/catch/mania | Nonstandard lazer mod settings currently return an explicit unsupported response |
| Rankings | Pending submissions stay out of rankings; moderators can review and mark scores verified; ordinary ranked PP excludes autoplay/relax/autopilot | Moderation is a human review policy, not automatic cheat detection |
| Replays | stable raw LZMA frames are bounded and decompressed for structural validation, then exported as an original `.osr`; stable downloads return raw LZMA and API downloads return the full container | Extended lazer replay uploads and deterministic replay simulation remain unsupported |
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

The reverse proxy must route Bancho hosts and `/web/*`, `/osu/*`, `/d/*`, `/oauth/*`, `/api/v2/*`, `/signalr/*` to Resonance. The spectator host routes `/multiplayer`, `/spectator` and `/metadata`, including WebSocket upgrades. Set `ILLYA_DOMAIN` for the restricted `a.<domain>` avatar endpoint. Forwarded IPs are trusted only when the immediate socket peer is in `TRUSTED_PROXY_CIDRS`. Query strings containing stable credentials are excluded from gateway request logs. Public score JSON never includes password hashes or protocol token hashes.

Run one Resonance gateway instance for the current realtime implementation. PostgreSQL retains rooms and private-channel memberships, while active spectator streams and transport queues live in memory. On restart, interrupted lazer rooms close and memberships are released; room history remains available.

Switching DNS or hosts entries only selects a server address. TLS trust, the correct protocol hostnames, API response compatibility, and native realtime transports must also work. Use the reversible Kaleidoscope connection helper and the [connection guide](connection.md); do not disable certificate verification to hide a deployment error.

## Validation evidence

On 2026-10-07, the independent live HTTP smoke test passed against a disposable PostgreSQL 18 instance, Sanctuary at `127.0.0.1:18080`, and Resonance at `127.0.0.1:18081`. It exercised stable binary login/ping/chat/logout, sender spoof prevention, shared authentication, lazer OAuth refresh revocation, staff-channel RBAC, solo-token identity binding, encrypted stable submission, checksum rejection, altered-replay rejection, server PP calculation, replay round trip, retry deduplication, and pending-to-reviewed ranking behavior.

The expanded live test also passed directed friends/blocks, friend-only DMs, password-protected stable rooms, non-host start rejection, rewritten authoritative score slots, ready/start/load/score/complete/host-transfer/leave transitions, and stable spectator frame relay. Native API checksum/set lookup, archive download and map score collections passed. After review, the fixture appeared in native rankings and its profile returned nonzero weighted PP, ranked score, global rank, combo and a level derived from verified total score.

The final gateway/protocol check passed 23 codec/model tests and 11 gateway unit tests, plus Clippy with warnings denied across all targets. These cover whole-batch packet rejection, every truncated packet/container prefix, secret redaction, bounded fields, password hiding, malformed realtime frames, level thresholds and avatar boundaries.

The separate `signalr_http_websocket_and_sse` integration test passed against an isolated PostgreSQL schema and a real ephemeral TCP listener. It exercised JSON and native-format MessagePack WebSockets, SSE negotiation/handshake/input/delete, room authority and gameplay transitions, typed spectator frames and timestamps, metadata, blocks and token revocation. It uses original synthetic fixtures, not an actual lazer executable or its loaded C# serializers. [Detailed evidence and the reproducible command](signalr-compatibility.md#reproducible-verification) record its exact scope.

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

Specific remaining features include stable Target Practice's extra score/replay field, in-editor map publishing, tournament interfaces, unknown filename resolution, extended lazer replay upload, arbitrary lazer mod settings and native chat notification delivery. Stable and lazer currently use separate multiplayer room models. Map score ordering uses submitted score totals; normalization between legacy stable totals and lazer's standardized totals needs a defined policy before combining competitive leaderboards at scale.

The realtime subset also excludes long polling, stateful reconnect, quick/ranked play, unsupported match types and countdown requests, per-room solo score aggregation and `UserScoreProcessed` notifications. These limits are enumerated in the [hub compatibility document](signalr-compatibility.md). No native stable or lazer build has completed the release checklist above.

## Interoperability sources

The legacy packet framing and field ordering were checked against [bancho.py packet definitions](https://github.com/osuAkatsuki/bancho.py/blob/master/app/packets.py). Login and legacy HTTP contracts were checked against its [Bancho endpoint](https://github.com/osuAkatsuki/bancho.py/blob/master/app/api/domains/cho.py), [legacy web endpoints](https://github.com/osuAkatsuki/bancho.py/blob/master/app/api/domains/osu.py), and [encryption boundary](https://github.com/osuAkatsuki/bancho.py/blob/master/app/encryption.py).

The lazer login contract was checked against the official client's [OAuth implementation](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/OAuth.cs). Solo score routes and payloads were checked against [CreateSoloScoreRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/Solo/CreateSoloScoreRequest.cs), [SubmitSoloScoreRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/Solo/SubmitSoloScoreRequest.cs), and [SoloScoreInfo](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/Responses/SoloScoreInfo.cs). Friend response fields follow the official [APIRelation model](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/Responses/APIRelation.cs).

Replay container facts were checked against the official [.osr file format](https://osu.ppy.sh/wiki/en/Client/File_formats/osr_(file_format)) and the [official legacy replay encoder](https://github.com/ppy/osu/blob/master/osu.Game/Scoring/Legacy/LegacyScoreEncoder.cs). Separate API, website and realtime endpoints are explicit in the official client's [EndpointConfiguration](https://github.com/ppy/osu/blob/master/osu.Game/Online/EndpointConfiguration.cs).

Profile level thresholds follow the official [total score and level description](https://osu.ppy.sh/wiki/en/Gameplay/Score/Total_score). The profile uses verified persisted score totals and weighted best-map PP rather than placeholder statistics; moderation remains the ranking policy.
