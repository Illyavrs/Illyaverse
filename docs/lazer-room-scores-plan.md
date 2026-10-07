# Lazer multiplayer room scores: implementation plan

This is a pending implementation plan. Resonance currently has room authority,
playlist state, loading/gameplay transitions, spectator transport, and the
native model codec, but it does not yet implement room score submission,
leaderboard HTTP responses, score aggregation, or `UserScoreProcessed`-style
notifications. Existing `ResultsReady` is only a room lifecycle signal; it is
not evidence that score processing has finished. No native room-score support
should be claimed until the checks below pass against the pinned client and a
real client session.

## Contract and identity

Pin the audit to osu! client commit
[`6cbcae7fbb764839a66c1adf564c2e1679242072`](https://github.com/ppy/osu/tree/6cbcae7fbb764839a66c1adf564c2e1679242072)
and record that identity beside the deployed protocol version. The primary
contracts are [`CreateRoomScoreRequest`](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Rooms/CreateRoomScoreRequest.cs),
[`SubmitRoomScoreRequest`](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Rooms/SubmitRoomScoreRequest.cs),
[`IndexPlaylistScoresRequest`](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Rooms/IndexPlaylistScoresRequest.cs),
[`MultiplayerScore`](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Rooms/MultiplayerScore.cs),
and the metadata contracts [`IMetadataServer`](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Metadata/IMetadataServer.cs)
and [`IMetadataClient`](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Metadata/IMetadataClient.cs).

Give every started round its own durable `round_id` and frozen participant
roster. A playlist item may be played again, so `(room_id, playlist_item_id)`
alone does not identify a round. Allocate playlist IDs monotonically and retain
their referenced snapshots after edits or removal. A score token must bind the
authenticated participant, room, playlist item, round, actual authorized beatmap
ID and checksum, ruleset ID, configured mod context and client version hash.
Freestyle choices must belong to the authorized set and use a supported
conversion. The server must reject a token used with another room,
round, user, map, checksum, ruleset, or score ID. Persist these bindings with
the token and score attempt so retries cannot silently move a result to a new
round. The authoritative playlist item retains its ID, map checksum, ruleset,
required/allowed mods, `expired`, `playlist_order`, `played_at`, star rating,
and freestyle flag as the pinned
[`MultiplayerPlaylistItem`](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Multiplayer/MultiplayerPlaylistItem.cs)
does. Its native MessagePack representation is the exact 12-element tuple
`[id, owner_id, beatmap_id, beatmap_checksum, ruleset_id, required_mods,
allowed_mods, expired, playlist_order, played_at, star_rating, freestyle]`.
`MultiplayerScore` and the room score HTTP responses are JSON contracts; do
not invent a MessagePack score map for them. The room JSON must continue to
carry `current_user_score` and playlist attempt summaries where the pinned
room models request them.

Create the room score token while the room is in `WaitingForLoad`, after the
server has selected and validated the active playlist item. It must remain
usable through loading, gameplay, and the result submission window, with a
bounded expiry tied to that round. A token created for a failed load or aborted
match is revoked. A reconnecting client may finish a still-valid attempt only
when the persisted binding and authenticated user match. Never issue a token
from `ResultsReady` or infer the active item from a mutable room setting.

## HTTP surface and recovery

Implement the following original server routes under `/api/v2` (with the
gateway's existing versioning/authentication rules):

* `POST /rooms/{room_id}/playlist/{playlist_item_id}/scores` creates one bound
  score token. Request fields are `version_hash`, `beatmap_id`, `beatmap_hash`,
  and `ruleset_id`; the response is `{ "id": ... }`.
* `PUT /rooms/{room_id}/playlist/{playlist_item_id}/scores/{score_id}` accepts
  the serialized score payload and returns the canonical `MultiplayerScore`.
* `GET /rooms/{room_id}/playlist/{playlist_item_id}/scores` returns the
  paginated `IndexedMultiplayerScores` shape (`scores`, `params`, optional
  `total`, and `user_score`). Support the pinned cursor parameters and stable
  ordering, with `scores_around` only when requested by the existing client
  contract.
* `GET /rooms/{room_id}/playlist/{playlist_item_id}/scores/{score_id}` and
  `GET /rooms/{room_id}/playlist/{playlist_item_id}/scores/users/{user_id}`
  return one canonical score after membership/privacy checks.
* `GET /rooms/{room_id}/leaderboard` returns the room-level leaderboard used by
  the pinned `GetRoomLeaderboardRequest`.

Submission must be idempotent on the bound token and payload digest. An identical
retry returns the original result, while a changed body conflicts. Reject room
tokens on solo routes and solo tokens on room routes. Reuse Sanctuary's existing
idempotent ingress key; its strict input does not accept invented room fields.
Never hold a room transaction across the service HTTP call. Atomically persist
the resulting score link, token consumption and room accounting after ingestion.
Reconciliation must recover the window where Sanctuary commits before Resonance
records the link, without creating another score. Revoked, expired or mismatched
attempts cannot expose another result; a consumed token still permits an exact
authenticated retry.

Define room processing separately from global moderator verification. The
proposed room policy lets frozen round participants view each other's processed
results through the exact authorized room/item binding, even while global
verification remains pending. Mark them unranked and retain their verification
state. Keep unrelated viewers excluded, and retain owner/moderator access under
the existing policy. Leaving, kicking and restart must not erase the binding or
silently make room history public. Rejected/restricted scores must obey the
documented result visibility rules rather than enter verified global rankings.

## Processing, aggregation, and notifications

Separate receipt, validation, and processing states. Validate native score
fields, map/ruleset/checksum, room membership, round identity, token ownership,
and replay binding before calculating or ranking. `ResultsReady` may be sent
when all active clients report finished, but it is not a processing barrier:
the UI and API must tolerate a short period in which a result is still pending.
Publish the processed event only after the canonical score and derived
statistics commit succeeds.

Count attempts when tokens are issued, including quit/no-score attempts. For
item highs, replace a result only for a strictly higher total; ties retain the
earlier score. Realtime head-to-head rooms may select failed scores as highs,
while playlist rooms require passing scores. Room totals sum item highs,
completed counts selected items, and accuracy averages those highs. Room ranks
sort by total descending, then highest selected score ID ascending. The pinned client
expects 13 total-score bins: ten 100,000-wide bins from 0 through 999,999,
two more bins for 1,000,000–1,199,999, and a final open bin for 1,200,000 or
more. Distribution and cumulative totals cover all passing processed attempts,
not only user highs. Failed, rejected, structurally invalid or duplicate scores
do not enter that distribution. Global pending moderation and PP exclusions
must not be confused with room processing or passing status. Include the last
processed score ID. A new-best rank is room/item scoped and nullable for a
non-best result.

The initial metadata snapshot returned by
`BeginWatchingMultiplayerRoom(room_id)` is an array of
`MultiplayerPlaylistItemStats`. Each item is the exact MessagePack tuple
`[playlist_item_id, total_score_distribution, cumulative_score,
last_processed_score_id]`. Later notifications use
`MultiplayerRoomScoreSetEvent`, the exact tuple
`[room_id, playlist_item_id, score_id, user_id, total_score, new_rank]`, where
`new_rank` is nullable. Emit it to currently watching, authorized metadata
connections only. Distribution updates are room/playlist scoped; do not send
solo-score statistics or fabricate a common distribution across rooms.

The spectator callback is separately `UserScoreProcessed(int userId, long
scoreId)`, with two scalar arguments. Send it to the submitting/ended spectator
connection, not every watcher. Register the ended connection after
`EndPlaySession` and check persisted processing state then, so processing before
registration is still delivered after the client's score watcher is ready.
An immediate score-PUT broadcast can race ahead of that registration. See the
[native spectator contract](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/Spectator/ISpectatorClient.cs)
and [official processing subscriber](https://github.com/ppy/osu-server-spectator/blob/66141f0e66678c685c9d3f266ff3cda5aabffb0e/osu.Server.Spectator/Hubs/Spectator/ScoreProcessedSubscriber.cs).

Metadata uses `MultiplayerRoomScoreSet` for authorized room watchers. The official
subscriber currently supports playlist rooms; using these events for Illyaverse
head-to-head rooms is an explicit extension. A closed connection may miss a push,
but a later `BeginWatchingMultiplayerRoom` receives committed statistics. Never
hold a database transaction while sending a WebSocket event or treat push failure
as failed durable processing. Preserve `ResultsReady` as the gameplay-state
signal: zero-hit/no-score plays and submission failures must still finish without
waiting for a score or moderator approval from every participant.

## Verification before implementation is considered complete

Add isolated PostgreSQL tests for token creation during `WaitingForLoad`, every
identity mismatch, expiry/revocation, duplicate identical submission,
duplicate conflicting submission, crash recovery after commit, retry after
disconnect, pending-result privacy, and the `ResultsReady`/processing race.
Test two users and two playlist items so room, round, user, and score IDs cannot
cross. Verify best-score replacement, attempt counts, all 13 bins, cumulative
passing totals, monotonic processing IDs, rank changes, failed HeadToHead best
scores, failed playlist exclusions, and rejected-score visibility. Add HTTP tests
for all five routes, cursor ordering and
privacy. Add JSON and MessagePack tests for both exact six-element event and
four-element stats tuples, including nullable `new_rank`, empty distributions,
and large score values. Finally, run a real pinned lazer client through token
creation, load, play, submit, reconnect, room result display, metadata snapshot,
and live score event delivery; record that evidence separately from synthetic
codec tests.
