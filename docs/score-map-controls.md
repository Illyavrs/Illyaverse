# Score replays and map review

Owners can attach a missing replay from `/scores/{id}`. Choose the matching non-empty `.osr` file, up to 16 MiB, then attach it. A failed upload keeps the selection for retry. The server checks the stored player, map, ruleset, mods and result binding; attachment does not verify gameplay. A new attachment returns the score to pending review. An identical existing replay retry preserves its current review status; a different existing replay cannot be replaced.

The website calls the existing owner-only `POST /api/v1/scores/{id}/replay`, with multipart field `file`. Moderators cannot upload on another player's behalf. A successful write is separate from a failed detail refresh: accepted feedback and a refresh action remain, and the website does not send the upload again. Current-account cache boundaries and API 401/403/404 handling remove inaccessible private detail and selected replay state.

Score detail distinguishes pending, approved and rejected results. Mod presentation uses native structured mods when supplied, otherwise the original legacy-bit display helper includes key counts and ScoreV2 (`SV2`). Unknown bits remain visible instead of appearing as NM. The acronym is checked against [osu-web's public mod metadata](https://github.com/ppy/osu-web/blob/master/database/mods.json); the rendering implementation is original.

Beatmap boards show the API's rank and comparison value: available standardised score for Normal, pp for RX. The original score remains separately visible. Scores with no available comparable scale are not converted by the browser; native/API ranking policy stays authoritative.

## Map review

The permission-gated administrative map tab supports submitted text search plus ruleset, source and status filters, a pending-community shortcut, counts and pagination. Its `tab=beatmaps`, `q`, `mode`, `source`, `status` and `page` URL state supports history and return links from difficulty details. The query is limited to 100 UTF-8 bytes; sequential filter changes merge the live URL to preserve a newly submitted search.

Each row has an explicit status save. Failed writes preserve the selected status and queue context. A saved change followed by a failed list refresh disables additional changes until refresh succeeds. Filtered last-page removal clamps only the current queue and refreshes its count. Other administrative tabs retain their existing permissions and behavior.

Public discovery also exposes official/community source selection. Public and administrative text search now match difficulty/version as well as title, artist and mapper, with the same parameterized predicate for count and items. Map detail offers a review entry only to staff with the relevant permissions and accepts only local administrative return targets.

These status choices affect ranking status. They do not quarantine, unpublish or delete content; no such action is advertised. Complete package replacement/version history and consistent native withdrawal remain separate work.

## Evidence boundary

Synthetic browser fixtures cover assembled file requests, limits, recovery, private-state clearing, ranking values and moderation navigation. Owned PostgreSQL request tests separately cover actual replay binding/idempotency/authorization and version search/filter/pagination/privacy behavior. The production-image regression and deployment evidence are recorded in `.impeccable/review/score-map-verification.md` after those checks complete. Fixtures do not launch stable/lazer or certify the pending real-client submission issue.
