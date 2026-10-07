# Score replay and map review verification

Scope: original local extensions of score detail, score tables, public map discovery and administrative map review. No migration, artwork or global design-system change. Native client score submission remains separately pending human verification.

## Source and persistence

- Full workspace source gate passed: 201 unit/CLI cases, 56 Resonance PostgreSQL request/integration cases and 66 Sanctuary PostgreSQL request/integration cases, workspace formatting and Clippy with warnings denied.
- The new version-search request case exercises case-insensitive/Unicode difficulty names, combined source/mode/status filters, counts, descending pagination, query length, parameterized input, public/admin parity, authentication and private-path redaction. Existing replay PostgreSQL cases exercise immutable binding, owner authorization, queued policy changes and idempotency.
- Website ESLint and TypeScript passed after the final UI source changes. All 32 infrastructure/migration/helper guards passed; all 19 historical migrations remain unchanged.
- The source gate used an owned fresh PostgreSQL fixture. Process-only Cargo/test environment was restored; Cargo incremental compilation was disabled for these runs. Compiler caches and retained workspace fixtures were not deleted.

## Browser and final images

- A disposable production-image browser regression passed **58/58**, including all eight new score/map cases, existing catalogue queues, normal/RX score detail, map package discovery/upload, official packages, community features and actual website/native HTTP/stable binary chat interoperability. Application images are listed below.
- The eight new cases use intercepted API fixtures: owner file validation and retained retry; accepted upload/failed refresh; non-owner and legacy/unknown mod presentation; access denial after acceptance; logout/private cache clearing; filtered final-page status updates; search/history/return links and Japanese mobile empty state; source selection and actual server-supplied comparison/rank. They do not certify gameplay or native submission.
- First assembled browser batch exposed a search/filter live-URL race and a download-wrapper geometry regression; both were corrected in the single source fix batch. The second root inspection opened all five final captures. Its test run passed 7/8; the remaining assertion counted an unrelated pending history refresh. Waiting for that actual response corrected test readiness without changing the UI image. The subsequent full regression passed 58/58 with captures disabled.
- Five full-page captures cover desktop dark and mobile light, zh-TW/en/ja, upload failure, accepted-write refresh failure, comparison ordering, filtered moderation and empty recovery. Exact dimensions, hashes and image identity are in `score-map-capture-metadata.json`. No third root visual hunt was performed.
- One detector run returned exit 0: **0 primary findings, 86 advisory findings**. Existing system drift is recorded separately and was not promoted into new design authority. Fresh generic agents substitute for unavailable dedicated finish-reviewer and documenter roles; their complete role instructions and output contracts were used. The fresh finish review returned **ship**, with no material fixes. Verdict and documentation are in the adjacent `score-map-verdict.md` and `score-map-documentation.md`.
- Owned final-image protocol smoke passed **349 HTTP requests / 42 check categories**, including encrypted stable submission fixtures, replay round trip, RX partitions and multiplayer lifecycles. Both disposable browser and protocol helpers verified removal of only their freshly owned Docker resources and temporary environments.

| Service | Verified image |
| --- | --- |
| Sanctuary | `sha256:2d0015b4e076b5d7440a0ab831ed6a33512e282c30792726d4afe4f62f5f800f` |
| Resonance | `sha256:3eaea5b830e6631a2b41da9cc7550652880ac21865232b0770b992d95fa4d059` |
| Serendipity | `sha256:70892ab7b7212488552bff30e8f538e190a90cb8972b6a6344132a0fb69a326c` |
| PostgreSQL | `sha256:1bea307dfb3ee30541a7acf7de14b58bcd6948da98e5d31a04c627c4d35ec64b` |
| Caddy | `sha256:f77f856a30f0004200b36b322d61da17fade31e24875699d77fb968399b9eb77` |

Ignored owned logs/helpers are retained under `data-score-map-controls/`.

## Retained local deployment

After the implementation and image gates passed, feature commit `9069844` was deployed to both existing stacks with the exact pins above. The native activity preflight was quiet (zero online stable/lazer sessions and zero received local-room receipts), and it was checked again after writers stopped.

Custom-format PostgreSQL dumps and shared-data tar archives were verified and retained under `data-score-map-controls/before-score-map-controls-20261007T212040Z-{main,client}.*`. No existing backup was overwritten. No migration ran: both databases retain successful versions 1–19 and no failed migration. The main stack retains two users, zero maps and zero scores; the client-test stack retains two users, 44 maps and zero scores. Complete account/profile/preferences, beatmap, filename-alias, message/private-message, relationship/read-marker and score fingerprints, plus official archive/member counts, are unchanged.

All ten running services are healthy and their application pins match the verified images. Both stacks' PostgreSQL and Caddy container IDs and image pins are unchanged. Post-update read-only HTTP checks passed for chat, channels and map catalogue; unauthenticated private conversations remain 401. HTTPS checks use only the existing CA as a request-local certificate and a request-local `--resolve`; trust and hosts are untouched. Runtime and deployment logs are retained in the same ignored evidence directory.

## Limits

Replay attachment checks result metadata binding and returns new attachments to pending review; it does not establish verified gameplay. Administrative status changes concern ranking status, with no quarantine/unpublish/delete promise. Official package replacement/version history and consistent native withdrawal remain separate work. Real stable/lazer gameplay and the human-reported stable submission failure are not proven by these fixture gates. No game process, hosts file, system trust, firewall or security setting was changed.
