# Beatmap ranking requests

Registered players receive `beatmaps.upload` and `beatmaps.rank.request`. New local uploads explicitly start in **Graveyard**. Historical map statuses and the legacy database insert default remain unchanged. The `beatmap_nominator` role has map import/moderation and request-review permissions; it does not grant account or score moderation. Authorization uses explicit permissions rather than role-name hierarchy.

This workflow is implemented in source. The owned schema and protocol receipts in the current integration index describe which checks have passed. A source implementation is not evidence that the retained client-test deployment has been upgraded or that a native client has exercised these endpoints.

## Player requests

All routes below use the `/api/v1` prefix and require an authenticated, unrestricted account.

| Route | Behavior |
| --- | --- |
| `POST /beatmap-rank-requests` | Submit a nomination with `native_set_id` or `official_set_id`, `reason`, and optional `difficulty_ids`. |
| `GET /me/beatmap-rank-requests` | Paginate the requester's requests; omitted status includes all states. |
| `GET /beatmap-rank-requests/{id}` | Return a request to its owner or a reviewer; another player receives 404. |
| `POST /beatmap-rank-requests/{id}/withdraw` | Withdraw the owner's pending request with its `expected_revision`. |

Creation requires `beatmaps.rank.request`. Losing this grant does not prevent the owner from reading or withdrawing an existing request. Lists support `page`, `page_size`, `status` (`all`, `pending`, `approved`, `rejected`, `withdrawn`), and an optional `native_set_id` filter.

Local requests must target the player's own active upload. Only Pending, Graveyard or legacy Unranked difficulties are eligible; published siblings can remain in the same set. An explicit selection must contain unique public native difficulty IDs from the verified set. Omitting the selection nominates all eligible difficulties for compatibility with older callers. A WIP difficulty must first be made Pending or Graveyard before nomination.

Any player can nominate an official Pending or Graveyard set. The existing official discovery cache, bounded response parser and global rate limiter verify its identity and difficulty checksums. Requests do not fetch an archive or publish invented `.osz` content. Their immutable manifest binds the selected native difficulty IDs to their checksums. Approval before content import records the individual server decisions; the normal later import inherits them.

Only one pending request may exist for a native set, including requests from different players. Each player may have five open requests and create ten requests within 24 hours. Actor locks and the database partial unique index enforce these limits under concurrent requests. Reasons are trimmed, contain 1–1000 characters, and reject control characters.

## Staff review and difficulty statuses

| Route | Permission and behavior |
| --- | --- |
| `GET /admin/beatmap-rank-requests` | `beatmaps.rank.review`; paginated queue, pending by default, with the same filters as the owner list. |
| `PATCH /admin/beatmap-rank-requests/{id}` | `beatmaps.rank.review`; decide `approved` or `rejected` with `expected_revision` and a reason. Approval also requires `beatmaps.moderate`. |
| `GET /admin/beatmapsets` | `beatmaps.moderate`; paginate genuine local uploads, including withdrawn sets. Supports `q` and `availability=all|active|withdrawn`. |
| `GET /admin/beatmapsets/{native_set_id}` | `beatmaps.moderate`; inspect all verified difficulties of a specific local or official set. |
| `PATCH /admin/beatmapsets/{native_set_id}/difficulties/{native_id}` | `beatmaps.moderate`; change one difficulty with a reason and both revision checks. |

Approval requires a nonempty `difficulty_ids` array and an equally sized `difficulty_revisions` array. Each revision entry contains `native_id`, `expected_revision` (the persistent override version), and `expected_status_revision` (the stored map's status version, or explicit `null` before import). These arrays must identify exactly the same unique selected IDs, and every selected ID must belong to the request's original manifest. Rejection accepts no difficulty selection. Approval ranks only the selected difficulties; it never ranks siblings implicitly.

The direct difficulty editor accepts `status`, `reason`, `expected_revision`, and `expected_status_revision`. Supported statuses are Pending, WIP, Graveyard, legacy Unranked, Ranked and Loved. Withdrawn sets can be inspected but cannot receive status edits until their independent availability state is restored.

The staff set DTO includes the persisted `availability` state (`withdrawn`, `revision`, `reason`, `changed_at`) and `representative_id`. A moderator or Beatmap Nominator can restore it through the existing `PATCH /admin/beatmapsets/{native_set_id}/availability` endpoint with `withdrawn:false`, the current `expected_revision`, and a reason. This requires `beatmaps.moderate` and does not require `admin.view`. Actor IDs and audit JSON remain absent from this DTO.

The legacy `PATCH /admin/beatmaps/{internal_id}` endpoint changes only that difficulty and records the same persistent native-ID override. It now requires both `expected_revision` and `expected_status_revision`; a stored map always has a numeric status revision. The public map DTO supplies these as `status_override_revision` and `status_revision`. Every status mutation checks both revisions and does not retry a stale decision automatically.

Override and stored-map revisions are independent. A new content import changes `expected_status_revision` from `null` to a number, and direct database status changes advance the stored-map revision. Checking both prevents a stale UI from silently overwriting an import or an A→B→A status change. Request revisions likewise advance on real workflow changes, including selected approval IDs. No-op or closed request decisions cannot manufacture review audits.

## Privacy, errors and transactions

The request DTO contains minimal public player identity, the private-to-owner/reviewer reason, review state, the selected difficulty summaries, reviewer identity and timestamps. It never exposes email, credentials, filesystem paths, package fingerprints, manifest checksums or private audit JSON. Other players cannot enumerate or read these requests.

`rank_request_exists`, `rank_request_conflict`, `rank_request_closed` and `rank_request_map_changed` return 409. `beatmap_status_conflict` returns 409 when either difficulty revision changed. `rank_request_limit` returns 429. Semantic eligibility and invalid-reason errors return 400; malformed JSON shapes use Axum's normal rejection response.

Mutations lock fresh role authority, the actor and session before the set fence and resource rows. They recheck the original session deadline before publication and commit. Decisions, individual statuses, persistent overrides and audit events commit together; a failed audit or stale selected difficulty rolls back the entire review. Official metadata failures leave requests, map content and status overrides unchanged.
