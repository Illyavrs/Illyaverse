# osu! client compatibility

Resonance is an original Rust implementation of the protocol boundaries. Reference projects were read to understand wire formats and API contracts; their server code was not incorporated. Compatibility is experimental until the release checklist has been exercised with actual client builds.

## Current coverage

| Area | Implemented behavior | Remaining validation or limitation |
| --- | --- | --- |
| Client launcher | Fixed Illyaverse Windows GUI; stable `-devserver`; mandatory original stock-lazer startup bridge, separate private storage, guarded game identity and owned plugin cleanup | Windows package and headless policy/lifecycle/runtime boundaries pass; actual native GUI, login, gameplay and exit remain user-operated validation. The bridge requires the exact tested stock build. See [launcher guide](kaleidoscope-launcher.md) |
| Registration | Website-only accounts; native `POST /users` and `/users/` consistently return structured errors with the configured website registration URL, without reading credentials or writing accounts/sessions | Actual stable/lazer dialog presentation still needs native client validation; website registration remains the supported creation flow |
| stable login | Three-line login, MD5 password verified against Argon2id, opaque `cho-token`, token hashes stored in PostgreSQL, 180-second polling session; legacy country lookup is case-insensitive (`TW` → 220, unknown → 244) | Native login/session timing and launcher `-devserver` routing still need validation; local DNS/CA preparation is a separate operator task |
| stable Bancho | Bounded binary framing, ping, status, presence, channel join/part, public chat, direct messages and friend-only DM settings | Actual native client packet timing remains to be validated |
| stable multiplayer | Durable 16-slot rooms, Argon2-protected passwords, authenticated host/slot authority, ready/start/load/score/complete, host transfer, room chat and leave/expiry cleanup | Tournament interfaces, advanced team result calculation and cross-client rooms remain unsupported |
| stable spectating | Authenticated watch/stop, peer notifications, bounded finite replay-frame bundles and spectator chat | Actual native replay playback/latency and disconnect timing require validation |
| Chat permissions and delivery | Authenticated native notification WebSocket, `chat.start`/`chat.end`, join/part/message events with sender models, native PM creation, history/read markers, UUID retry deduplication, keepalive and shared website/stable messages | Actual lazer UI behavior and reconnect timing remain unverified; announcement channels and non-chat notification types remain unsupported. See [native chat contracts](native-chat.md) |
| stable score submission | Bounded multipart with case-insensitive disposition parameters, encrypted score envelope, Rijndael with 256-bit blocks and CBC, strict padding, checksum, login/client hash binding, authenticated map identity, idempotency | Real-client 413 was observed; corrected attachment parsing passes isolated encrypted submission tests, with actual persistence still pending. Structural integrity is not a gameplay verdict. See [submission diagnostics](native-score-submission.md) |
| PP | Sanctuary calculates performance using rosu-pp from the stored `.osu`, complete judgments/ticks, combo, ruleset and validated structured GameMods; supported speed, DA/CL/HD and ruleset settings persist as `api_mods`; standard maps can convert to taiko/catch/mania | Unknown, duplicate, incompatible, out-of-range and unsupported settings are rejected. See [native mod policy](native-mods.md) |
| Rankings | Pending submissions stay out of rankings; moderators mark reviewed scores verified; valid RX uses separate PP, ranks and statistics for osu!/taiko/catch; DA/AP/AT exclusions remain and native filters compare validated mods | Mania RX is rejected. Historical RX needs explicit audited recalculation. Native display/gameplay still requires player validation; moderation is a human review policy. See [RX policy](relax.md) |
| Replays | Stable raw LZMA frames are validated and exported as `.osr`; lazer spectator captures persist bounded chunks, bind to score metadata, recover pending exports and support native binary downloads plus owned extended `.osr` uploads | Deterministic replay simulation and actual game-host playback remain unverified; interrupted captures are disclosed and stay pending. See [lazer replay contracts and native DLL evidence](lazer-replays.md) |
| Local maps | Immutable upload packages, same-owner complete exact retries, canonical native difficulty/set IDs, grouped osu!direct/lazer search, sibling lookup, original complete `.osz` download and website paged membership/upload results with explicit file kinds; new uploads also retain exact filename aliases and validate raw ZIP records with UTF-8/CP932/Unicode Path decoding | Package source/image/browser checks and the separate filename/schema-0017 checkpoint are recorded in verification. Native-client checks remain pending. New local content must have no positive embedded official IDs; legacy native URL refresh and retained positive-ID limitations are explicit. Legacy filename backfill and mixed stable filename/ID indexing remain incomplete; a standalone `.osu` has no `.osz`. Strict malformed-encoding policy is distinct from stock reader behavior. See [package contract](local-map-packages.md) |
| Official maps | Authenticated stable selection/submission and lazer checksum/ID lookup/token creation resolve unknown maps through Sanctuary's OAuth-backed lookup, bounded download, cache and retry handling; filename-only lookup has an exact selector. Native set browsing can obtain bounded complete original metadata without publishing map files; matching stored moderation is merged freshly. Staff can explicitly upload a verified complete official archive with immutable publication, original downloads and website membership | Requires the operator's own osu! OAuth credentials; automatic whole-set content/asset downloads, multiple versions per official ID, official multi-parameter fallback and Qualified parity remain incomplete. Set metadata is separate from individual content validation and archive availability. Official IDs in the service's managed local range are refused; actual native-client certification remains pending. See [native discovery](native-beatmap-discovery.md) and [official archives](official-beatmap-archives.md) |
| lazer OAuth | Password grant, refresh grant, separate opaque access/refresh tokens; refresh rotation invalidates both old tokens; user restrictions rechecked | Client identifiers are treated as public native-app identifiers; no official client secret is embedded |
| lazer solo scores | Native score-token route and JSON submission, authenticated user/checksum/canonical-map/ruleset binding, post-discovery bearer recheck, retry deduplication, persistent submission statistics | Accuracy is labeled client-reported; PP uses complete supplied judgments/ticks |
| lazer room scores | Original source for frozen rounds, room-bound tokens, durable submission recovery, authorized result/leaderboard routes, native totals/attempts/distributions and room/spectator processing events | HeadToHead creation only. Four isolated PostgreSQL/HTTP/WebSocket checks and 19 pinned native-DLL DTO assertions pass; real native gameplay/result display remains pending. [Room-score contract and checklist](lazer-room-scores-plan.md) |
| lazer API | Current user/profile with verified database statistics, checksum lookup, official/local ID resolution, filename-only exact alias/discovery lookup, beatmapset lookup/search/archive download, score collections/details/download, public chat REST and room membership checks | Full official API parity and actual native validation remain incomplete; checksum/ID binding intentionally stays strict, and untracked play time and replay-watch metrics are omitted |
| Friends and blocks | Durable directed relationships, stable friend packets, lazer relation API, blocked-player DM enforcement and shared website controls | Each mutation requires the current `social.manage` permission |
| lazer realtime | SignalR JSON/MessagePack codecs, authenticated WebSocket/SSE routes and room/spectator/metadata subsets | Real transport tests are separate from the native client release checklist; a single gateway owns transient streams. See [detailed hub contracts](signalr-compatibility.md) |

Unimplemented endpoints must not be advertised as completed features. A successful synthetic request proves the tested server contract, not full native-client compatibility.

## Stable protocol boundary corrections

The stable packet and legacy HTTP corrections are covered by focused tests. A valid empty `PING` (packet type 4) drains queued packets and does not cause an unsolicited `PONG` (type 8). The independent initial-response parser checks `cho-protocol: 19` and consumes each binary packet completely, including self ID, presence, stats, friends and channel sections. Repeated idle polls remain empty, a queued notification is delivered once, and a nonempty `PING` payload returns 400. The legacy HTTP checks cover `GET /web/osu-getseasonal.php` returning the JSON array `[]` and `GET /web/bancho_connect.php` returning an empty 200 response. The legacy country lookup is case-insensitive (`TW` → 220; unknown → 244). For an existing visible account with no avatar, the avatar endpoint returns an original opaque black 64×64 PNG and supports its ETag/304 path; unknown or restricted accounts return 404. See the [social session parser tests](../services/resonance/src/social_session_tests.rs) and [protocol smoke test](../services/resonance/tests/protocol_smoke.py).

## Deployment contract

Resonance listens on `RESONANCE_BIND` (default `0.0.0.0:8081`) and shares PostgreSQL migrations with Sanctuary. Set `DATABASE_URL`, `STORAGE_PATH`, `SANCTUARY_INTERNAL_URL`, and an `INTERNAL_SERVICE_KEY` of at least 32 bytes. Sanctuary and Resonance must mount the same maps/replays storage. Only hashed protocol tokens are persisted; the website session cookie uses a separate session format.

The reverse proxy must route Bancho hosts and `/users`, `/users/`, `/web/*`, `/osu/*`, `/d/*`, `/oauth/*`, `/api/v2/*`, `/signalr/*` and `/notification-server` to Resonance. The spectator host routes `/multiplayer`, `/spectator` and `/metadata`, including WebSocket upgrades. The native chat URL comes from `LAZER_NOTIFICATION_ENDPOINT`, or the configured public origin with the WebSocket scheme and `/notification-server` path; it cannot contain credentials, query parameters or fragments. Set `ILLYA_DOMAIN` for the restricted `a.<domain>` avatar endpoint. Forwarded IPs are trusted only when the immediate socket peer is in `TRUSTED_PROXY_CIDRS`. Query strings containing stable credentials are excluded from gateway request logs. Public score JSON never includes password hashes or protocol token hashes.

Run one Resonance gateway instance for the current realtime implementation. PostgreSQL retains rooms, private-channel memberships and score-token replay capture chunks; live spectator fanout and transport queues use memory. On restart, interrupted lazer rooms close and memberships are released; room history remains available. Ended replay captures already bound to scores recover in bounded startup/periodic batches.

Players use the fixed-server [Kaleidoscope launcher](kaleidoscope-launcher.md): stable starts with `-devserver`, and supported stock lazer starts with the temporary Transfiguration bridge. The player GUI does not edit hosts. Operators prepare public DNS/TLS, or isolated local DNS/CA for `illya.test`, according to the [connection guide](connection.md). DNS or hosts routing alone does not set lazer OAuth/API endpoints or establish protocol compatibility; keep certificate verification enabled. The current test package is fixed to `illya.test`, so a public deployment also requires an updated and validated fixed-endpoint package.

## Validation evidence

On 2026-10-07, the independent live HTTP smoke test passed against a disposable PostgreSQL 18 instance, Sanctuary at `127.0.0.1:18080`, and Resonance at `127.0.0.1:18081`. It exercised stable binary login/ping/chat/logout, sender spoof prevention, shared authentication, lazer OAuth refresh revocation, staff-channel RBAC, solo-token identity binding, encrypted stable submission, checksum rejection, altered-replay rejection, server PP calculation, replay round trip, retry deduplication, and pending-to-reviewed ranking behavior.

The expanded live test also passed directed friends/blocks, friend-only DMs, password-protected stable rooms, non-host start rejection, rewritten authoritative score slots, ready/start/load/score/complete/host-transfer/leave transitions, and stable spectator frame relay. Native API checksum/set lookup, archive download and map score collections passed. After review, the fixture appeared in native rankings and its profile returned nonzero weighted PP, ranked score, global rank, combo and a level derived from verified total score.

The final gateway/protocol check passed 23 codec/model tests and 11 gateway unit tests, plus Clippy with warnings denied across all targets. These cover whole-batch packet rejection, every truncated packet/container prefix, secret redaction, bounded fields, password hiding, malformed realtime frames, level thresholds and avatar boundaries.

The separate `signalr_http_websocket_and_sse` integration test passed against an isolated PostgreSQL schema and a real ephemeral TCP listener. It exercised JSON and native-format MessagePack WebSockets, SSE negotiation/handshake/input/delete, room authority and gameplay transitions, typed spectator frames and timestamps, metadata, blocks and token revocation. It uses original synthetic fixtures, not an actual lazer executable or its loaded C# serializers. [Detailed evidence and the reproducible command](signalr-compatibility.md#reproducible-verification) record its exact scope.

The `native_chat_http_and_websocket` test passed against a separate random PostgreSQL 17 schema and a real ephemeral HTTP/WebSocket listener. It exercised the native notification URL and event shapes, sender/UUID echoes, deferred commits, concurrent retries and opposite PM sends, friend-only/block rules, channel history/read markers, keepalive expiry, private-room and channel membership revocation, stable/native binary chat interoperability, repeated start/end/resume and session expiry/revocation/restriction. Four chat unit tests also passed. [Native chat evidence](native-chat.md#verification) records the command and limits; no native game application was launched by these tests.

The current combined source gate passed 175 unit/CLI tests, 30 ignored Resonance tests and 12 ignored Sanctuary tests (40 PostgreSQL integration tests plus 2 existing native-artifact vectors), with formatting and Clippy warnings denied. The current isolated final-image HTTP smoke passed 312 requests across 36 categories against PostgreSQL 17, using Sanctuary image `sha256:f7b971edac07e02698c047ea0aacbcd042e8949e71add9b2df625dbe52ba94e1` and Resonance image `sha256:73ed182783554e5d054145e2a69fbb6fe70b42ad5c6a9d4d1382174abd39f1b9`. Local ignored evidence is retained in `data-stable-submit/source-gate-final.log` and `data-stable-submit/final-http-smoke.log`. The [upload and discovery checkpoint](verification.md#native-upload-and-lazer-discovery-checkpoint) records the new resolver/attachment tests and limitations. These server and protocol checks do not certify a native gameplay pass.

The score cipher fixture is independently encrypted by the test-only Python `py3rijndael` library. The Rust implementation uses `simple-rijndael` for the block primitive and original framing/CBC/validation code. This prevents the test from merely encrypting and decrypting with the same implementation.

Stable multipart read failures preserve the pinned Axum 0.8.9 [MultipartError status contract](https://docs.rs/axum/latest/axum/extract/multipart/struct.MultipartError.html#method.status) in both initial-field and field-chunk reads. Framework body limits use 413 `score_upload_too_large`; malformed/truncated input uses 400 `invalid_score_multipart`; other framework stream failures keep their provided status with the same static generic code. Raw parser details never become response/log text. Existing scalar, replay, aggregate and framed-body limits remain unchanged. The actual recorded part-14 scalar-limit 413 has a separate, still-pending native diagnostic; status preservation alone does not establish a successful real-client submission.

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
- Confirm global pending scores remain private to the owner/moderators and appear in rankings only after review; processed room results additionally permit peers from the exact frozen round through room routes.
- Validate HeadToHead room token creation while loading, submission/retry, result display, authorized metadata statistics and ended-connection processing events.
- Validate native realtime transport negotiation, rooms, spectators, host transfer, ready/load/complete and disconnect recovery.
- Check TLS trust and reversible host changes, including restoration after the helper is removed.

Specific remaining features include stable Target Practice's extra score/replay field, in-editor map publishing, tournament interfaces, legacy filename backfill and mixed stable filename/ID indexing, unsupported lazer mod settings, announcement channels and non-chat notification types. Stable and lazer currently use separate multiplayer room models. Native map score leaderboards compare reported lazer standardized totals with passed, convertible stable estimates; unsupported score scales are excluded. Stable map boards preserve their raw-score source. Shared profile totals use Classic display caches for native/ScoreV2 and raw stable V1 history; ranked totals select a standardized best result per map before adding its Classic value. PP selection remains independent. The original versioned converter is contract- and integration-tested, with actual gameplay still pending; see [score normalization](score-normalization.md).

The realtime subset also excludes long polling, stateful reconnect, quick/ranked play, unsupported match types and countdown requests. Room-local aggregation and `UserScoreProcessed` delivery have original implementations for bound room attempts, separate from solo/global rankings; the isolated room-score gate passes. These limits and statuses are enumerated in the [hub compatibility document](signalr-compatibility.md). No native stable or lazer build has completed the release checklist above.

## Interoperability sources

The legacy packet framing and field ordering were checked against [bancho.py packet definitions](https://github.com/osuAkatsuki/bancho.py/blob/master/app/packets.py). Login and legacy HTTP contracts were checked against its [Bancho endpoint](https://github.com/osuAkatsuki/bancho.py/blob/master/app/api/domains/cho.py), [legacy web endpoints](https://github.com/osuAkatsuki/bancho.py/blob/master/app/api/domains/osu.py), and [encryption boundary](https://github.com/osuAkatsuki/bancho.py/blob/master/app/encryption.py).

The lazer login contract was checked against the official client's [OAuth implementation](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/OAuth.cs). Solo score routes and payloads were checked against [CreateSoloScoreRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/Solo/CreateSoloScoreRequest.cs), [SubmitSoloScoreRequest](https://github.com/ppy/osu/blob/master/osu.Game/Online/Solo/SubmitSoloScoreRequest.cs), and [SoloScoreInfo](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/Responses/SoloScoreInfo.cs). Friend response fields follow the official [APIRelation model](https://github.com/ppy/osu/blob/master/osu.Game/Online/API/Requests/Responses/APIRelation.cs).

Replay container facts were checked against the official [.osr file format](https://osu.ppy.sh/wiki/en/Client/File_formats/osr_(file_format)) and the [official legacy replay encoder](https://github.com/ppy/osu/blob/master/osu.Game/Scoring/Legacy/LegacyScoreEncoder.cs). Separate API, website and realtime endpoints are explicit in the official client's [EndpointConfiguration](https://github.com/ppy/osu/blob/master/osu.Game/Online/EndpointConfiguration.cs).

Profile level thresholds follow the official [total score and level description](https://osu.ppy.sh/wiki/en/Gameplay/Score/Total_score). The profile uses verified persisted score totals and weighted best-map PP rather than placeholder statistics; moderation remains the ranking policy.
