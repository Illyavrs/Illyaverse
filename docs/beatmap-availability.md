# Beatmap-set availability

This document describes the implemented source contract for reversible withdrawal and restoration. It does not establish that a deployment has been upgraded or that a native stable/lazer client has passed testing. Consult the [verification record](../.impeccable/review/beatmap-withdrawal-verification.md) for completed gates, image versions and outstanding checks.

## Availability and ranking status

Availability belongs to the canonical `native_set_id`, rather than an individual difficulty or an internal database row ID. Withdrawing a set covers all its difficulties, including official difficulties that have not yet been cached. An official set can therefore be withdrawn before it has any stored members. A local set must already have a package before its availability can be changed.

Ranking status remains independent. `pending`, `ranked`, `loved` and `unranked` difficulties are discoverable while their sets are active. Changing a difficulty's ranking status does not restore its set, and restoring a set does not change its ranking statuses.

Withdrawal excludes the set from public discovery, lookup, downloads, new score admission, new playlist selection and new rounds. It retains packages and files, IDs and hashes, filename aliases, existing scores, replays, review decisions, tokens, frozen room rounds and audit history. Historical results and replays remain subject to their existing access and privacy rules. Retained native history describes withdrawn content as unscoreable with downloads disabled.

Qualified best performances and ranked PP/rank totals are calculated from active maps. Withdrawal removes the set's eligible contributions without rewriting saved scores; restoration makes the retained qualified results eligible again. Lifetime play counts and score/history totals remain retained. Restoration alone does not approve a score or change its verification state.

An ordinary import, a retry of the same package, or an official cache hit cannot restore a withdrawn set. Restoration is an explicit availability transition.

## Administrative access

| Operation | Permission | Endpoint |
| --- | --- | --- |
| List maps, including retained withdrawn maps | `admin.view` | `GET /api/v1/admin/beatmaps` |
| Read retained map details and the moderation reason | `admin.view` | `GET /api/v1/admin/beatmaps/{id}` |
| Withdraw or restore a whole set | `beatmaps.moderate` | `PATCH /api/v1/admin/beatmapsets/{native_set_id}/availability` |

The detail endpoint uses the internal map row `id`; the availability endpoint uses the canonical `native_set_id`. Obtain the latter from the map metadata instead of substituting a difficulty ID.

The administrative list accepts `availability=active`, `withdrawn` or `all`, with `active` as the default. This filter combines with the existing query, mode, source, ranking-status and pagination filters. For example:

```text
GET /api/v1/admin/beatmaps?availability=withdrawn&source=official&page=1&page_size=24
```

`admin.view` grants retained metadata access; it does not grant availability mutation or bypass the public download gate. Unauthenticated administrative requests return `401`, and authenticated users lacking the required permission receive `403`. The mutation rechecks the actor's current authority after waits and before commit.

To operate through the website:

1. Open the administrative beatmap list and select the availability filter needed to locate the set.
2. Open its management detail, check the set ID and the listed difficulties, then choose Withdraw or Restore.
3. Enter a factual reason and confirm the whole-set change. The action includes uncached official members, rather than only the displayed difficulties.
4. If the state changed concurrently, refresh it before submitting another decision. If a change was acknowledged but a subsequent page refresh failed, refresh the page rather than repeating the acknowledged mutation.

The administrative API also supports withdrawal of an uncached official set using its known canonical set ID. Such a set has no cached difficulty row to display in the administrative map list.

## Mutation contract

The request requires all three fields and rejects unknown fields:

```json
{
  "withdrawn": true,
  "expected_revision": 0,
  "reason": "Temporary review of this fixture set."
}
```

Send this example to `PATCH /api/v1/admin/beatmapsets/123456/availability` with the normal authenticated website session and request-origin protections. The ID and reason above are synthetic examples.

`expected_revision` must be nonnegative. A set without an availability record is active at revision `0`, with `reason` and `changed_at` both null in the administrative projection. A real transition increments the revision by one. The reason is trimmed and must contain 1–500 characters, fit within 2,000 UTF-8 bytes and contain no NUL character. These validations also apply to requests whose target state already matches the current state.

A successful transition returns `200` and the committed state:

```json
{
  "native_set_id": 123456,
  "availability": {
    "withdrawn": true,
    "revision": 1,
    "reason": "Temporary review of this fixture set.",
    "changed_at": "2026-01-01T00:00:00Z"
  }
}
```

Use the returned revision for the next decision. A restoration request following that example is:

```json
{
  "withdrawn": false,
  "expected_revision": 1,
  "reason": "Fixture review completed."
}
```

Restoration returns revision `2`. The availability record remains stored after restoration; it does not revert to an absent revision-zero state.

The endpoint uses compare-and-swap semantics. A stale `expected_revision` returns `409` with error code `availability_conflict`, even if the requested boolean now matches the current state. Reload the authorized metadata, review the new state and then make a fresh decision using its revision.

A request with the current revision and the same `withdrawn` value is an idempotent no-op. It returns the existing state and preserves its reason, timestamp and revision. It creates no additional audit or native update event. This endpoint does not provide reason-only edits.

## Public behavior and privacy

Public catalogues remain active-only. Supplying `availability=withdrawn` or `all` to the public list does not enable administrative discovery. Known withdrawn map lookup and download requests return `410 Gone` with code `beatmap_withdrawn`, instead of being treated as a missing map eligible for official rediscovery. Native routes enforce the same durable availability authority through their protocol responses.

The public availability projection contains `withdrawn`, `revision` and `changed_at`. It omits the moderation reason and actor identity. Administrative list and retained-detail responses add `reason`; access to those responses requires `admin.view`. An error response explaining withdrawal does not expose that private reason.

## Scores, retries and history

New score publication is denied while the set is withdrawn. Admission is checked again inside the publication transaction after any set-lock wait, so a successful earlier lookup or gateway preflight does not authorize a later score insert.

An exact, already persisted score retry can return or finish linking the existing result after withdrawal. This is an idempotency exception for the saved ingestion context, not permission to submit another score. A different ingestion key, player, map, ruleset or score variant cannot reuse it. Existing authentication, session, token expiry, revocation, immutable submission binding and partition checks continue to apply.

Saved receipts compare immutable semantic input: authenticated transport/player, map, ruleset/variant, raw score, combo, mods, passed state, every normalized judgement counter, and accuracy. Lazer also binds normalized structured mods and the native submission context. Stable accuracy is derived from its judgements; stored float8 values use an absolute tolerance of `1e-7` percentage points. Missing optional legacy counters normalize to zero. A key collision with different input returns `409`, including when the saved score belongs to a withdrawn set. Calculated PP, review state, ranking status and current difficulty attributes do not redefine the saved input or invalidate an otherwise exact retry. History remains retained under its existing access and privacy rules.

Stable's legacy checksum can be identical for different `300`/`100` distributions. The individual counters are therefore compared independently: changing them under the same checksum cannot return the saved receipt or backfill its missing replay. The gateway rechecks the final saved row after Sanctuary HTTP/publication waits and before attachment or response. An existing replay must also match its immutable header identity and exact compressed frames. A legacy row without a replay has no independent stored time/version/perfect/rank metadata; these remain bound by the already validated online checksum and client/session checks. Older replay containers may omit checksum/online ID without dropping the stored score comparison.

Received but unpublished lazer intents and room receipts are retained and deferred with `beatmap_withdrawn` rather than discarded solely because of withdrawal. A denied issued token does not acquire a new accepted submission. Restoration allows normal processing to be considered again; it does not extend token/session expiry, undo revocation or bypass binding/security validation. An expired or otherwise invalid retained intent can therefore still fail or be revoked under the existing rules after restoration.

Existing frozen rounds and result/replay history remain retained. Their presence does not authorize creation of a new round or new score publication for withdrawn content.

## Atomicity and verification

Each actual transition commits its state change, one `beatmaps.withdraw` or `beatmaps.restore` audit entry, and one native beatmap-set update event atomically. The audit targets the canonical beatmap set and records before/after state, reason and revision. A rolled-back transaction publishes none of these changes. No-op requests publish none of them either.

Durable database state is authoritative over metadata caches and upstream responses. Shared publication/admission fences and exclusive availability transitions use the same canonical set identity, with a fresh availability check after waits. Network ingestion does not hold these database fences across its HTTP request; final publication still passes the database gate.

The latest workspace/unit and isolated PostgreSQL service gates passed, including exact receipt and missing-replay regressions. The rebuilt final production images also passed the full isolated protocol smoke: 438 HTTP requests across 51 check categories. Related website regression and retained deployment are recorded separately in the verification record. Real stable score submission remains unresolved; source and isolated integration evidence do not establish compatibility with every original stable/lazer client.

The [implementation plan](beatmap-withdrawal-plan.md) describes the cross-service boundaries and the isolated `npm run test:availability:db` gate. That gate covers schema retention, both identity namespaces, ranking statuses, restoration, saved versus new score publication, atomic event rollback and real database lock waiters. It is not a substitute for service integration tests, browser checks, final-image/runtime verification or actual native client testing. The [verification record](../.impeccable/review/beatmap-withdrawal-verification.md) records evidence for those distinct stages.
