# Map browsing correction — 2026-10-09

The user's catalogue and home-hover screenshots override the previous visual review. This correction preserves the black default background, independently adjustable glass coverage/blur/opacity/radius, factual copy, shadcn controls, and the existing typography. No generated artwork is used.

## Behavior

- The catalogue toolbar has one section material layer. Inputs and Select triggers use thin neutral veils without their own blur. Full coverage no longer adds a fixed 92% black reading scrim; excluded section levels remain solid and readable.
- Catalogue cards reserve equal title/artist/mapper/facts tracks and eight bounded difficulty slots. One, two and eight difficulties align at the same viewport. Desktop grids retain aligned preview tracks; phone previews use a compact horizontal rail rather than several empty rows. Grid/list views retain keyboard access and 44px difficulty targets.
- Map detail identity and selected-difficulty facts form a joined overview, retaining the complete difficulty selector and leaderboard.
- Normal player downloads are `.osz`. A retained package uses `/api/v1/beatmaps/{id}/archive`, which validates an actual archive and never substitutes a difficulty file. A missing retained archive fails explicitly. Official difficulties without a retained package link to the verified osu.direct complete-package provider and disclose that source; the official set page remains available. A raw-only local set disables the package action and explains why.
- Legacy raw `.osu` endpoints remain available to client protocol/editor workflows. No fake ZIP assembled from a single difficulty is offered as a complete package.
- Home map hover and keyboard focus use a thin neutral veil and an inset outline/divider without adding blur or moving the row. Content insets are 20px on desktop and 16px on phones, preserving cover and number spacing inside the full-width interaction area.

## Reference study

The implementation is original. The osu-web card and detail source at revision `04643dee6cacfaadc6c80976256481322e21e945` informed bounded metadata tracks, cover/identity separation and difficulty overflow handling. The osu-web download controller and Sunrise Direct controller informed the distinction between complete package delivery and raw difficulty lookup. The current provider route was independently checked against [osu.direct's browser extension](https://github.com/osu-direct/browser-extension/blob/6960efd43ba9dfff95717a8a9807f9e4deb9455d/src/background.ts).

## Verification

Verification uses fresh, owned disposable PostgreSQL/browser harnesses. Retained preview/client-test records are only read. Captures use synthetic test data; upstream official covers are rendered in the browser with fallback, and QA scene backgrounds are test fixtures, not default shipped artwork. Desktop, the user's 1288px width, 390px light English and 320px dark Japanese are included, alongside glass coverage 0/50/100 and a custom-material home hover.

The final isolated browser gate passed all 70 tests in the five affected suites (`home-community`, `map-discovery`, `beatmap-withdrawal`, `map-packages`, `official-map-packages`). It covers equal card tracks with long metadata and varying difficulty counts, all eight phone links through native Tab navigation and local rail visibility, grid/list return state, desktop/mobile light/dark material coverage 0/50/100, single-layer toolbar blur, homepage hover/focus contrast and real content insets, package availability and withdrawal. The owned harness `illya-web-smoke-7a559bec947e2e16` was removed and cleanup verified. This is the final-image affected-suite gate, not a claim that the entire browser suite ran on that image.

The strict archive selection unit test and four owned PostgreSQL 17 integration cases passed. They verify original archive bytes and supplied assets, explicit 404 for absent archives with the legacy raw endpoint still available, complete local/official package membership and withdrawal preserving identity/history while rejecting downloads. The exact owned container and volume for `illya-map-package-api-3c465a1f7329ad22988254ab` were removed; no resources with its ownership label remain. Sanctuary formatting and all-target Clippy with warnings denied passed. Frontend ESLint, TypeScript and the production build passed. Workspace-wide formatting still reports the pre-existing unrelated layout in `crates/illya-domain/src/lib.rs:249`; it was not changed here.

The external provider was checked with a four-byte ranged GET: HTTP 206, range `0-3/1863302`, ZIP signature `504b0304`. This verifies the current package route without claiming a complete download or native client import.

Final exact images:

- Serendipity: `sha256:0bf9bef63031f3e5d2159409784c5f8340ad82acb349f22a5eb949f8f6e1567a`.
- Sanctuary: `sha256:ee11f88c67c8168edd82677a536db9bf40839931fc400b5f816b3339b8847ee6`.

Local evidence is retained under ignored `data-map-card-correction-20261009`: `browser-gate-final.log`, `web-build-final.log`, `api-build.log`, `api-unit.log`, `api-integration-gate.log`, `api-integration-3c465a1f7329ad22988254ab.json`, `provider-range.log` and `capture-evidence-20261009T023923Z/capture-evidence.json`. The capture packet validates the hashes, dimensions, freshness and provenance of 16 required current PNGs. Screenshots remain labelled synthetic fixtures, not retained server data.

The detector ran once on the four changed UI files. Its 51 findings are advisory type/radius scale checks (45 type and six radius), assessed against the existing documented compact record styles and independently configurable geometry. They do not substitute for the image review. The unavailable specialized Impeccable reviewer/documenter roles use the supplied degraded role contracts in fresh default agents. The documenter checked the final source and preserved normative tokens, extensions and all ten component previews.

The fresh independent finish reviewer returned `disposition: ship` after opening both user anti-reference screenshots and all 16 current PNGs, then checking source and provenance. Its five-section report records matching material/card/detail/home behavior and no material fixes. The ignored complete report is `finish-review.md`. This verdict covers this narrow correction only; the initial reviewer was interrupted before a report and its preliminary observation was not treated as a verdict.

## Deployment and actual retained data

Docker Desktop was stopped when work resumed. Starting the installed application restored all ten retained containers. A separate read-only restart proof records the operating-system launcher creation time, the permitted one-time start/finish timestamp changes, unchanged full container/image identities, existing named-volume ownership/creation dates, mounts, schema22 fingerprints and every prior retained data ID. Historical manifests did not contain complete mount inventories; this limitation is recorded rather than inventing a historical comparison. The validated current mounts and nanosecond start times were frozen for the continuation. The original deployment script and prior manifests were not changed. All 71 offline continuation checks and its actual read-only preflight passed.

The tested Web/API images were deployed to client-test, then main, recreating only Sanctuary and Serendipity. Both stacks passed: all ten containers healthy, all six PostgreSQL/Resonance/Caddy identities and frozen start times preserved, named volumes and bind files retained, schema22 and original data IDs unchanged. Client-test retains two users, 48 maps, two scores and one Replay; main retains two users with no maps/scores/Replays. No native-client, preference, trust, hosts, firewall or database restore changes were made.

Actual read-only browser inspection at `https://illya.test` confirmed all 24 catalogue cards on page one are 323px tall at 1280px, no document overflow, a single 8px toolbar blur with no controls/field blur, the joined map42 detail with the disclosed `.osz` provider and official-page links, and homepage rows with 20px horizontal insets and no row blur. Map42 still displays approved score2 at 19.78 pp. Trusted HTTPS checks return 200 for map42, score2 and its 13,047-byte Replay. Since this existing official map has only a raw difficulty retained locally, its strict archive route correctly returns 404 while its protocol `.osu` route returns 200; the normal visible package action uses the complete provider instead.

Immutable local deployment evidence: `deploy-after-daemon-restart-20261009T044120Z-aa4c0272bbe443418a8fb6c38499a12b/deployment-manifest.json`. Actual public HTTP/browser observation evidence: `runtime-readonly-verification-20261009T044551Z-1f5c0913ed354334bd228e02f007e441.json`. Both reside under ignored `data-map-card-correction-20261009`, separate from synthetic browser screenshots.

Native game download/import behavior is not claimed by the HTTP and browser checks.

The user's subsequent screenshots request a substantially denser, consistently compact search/card layout and a supplied transparent character in the homepage hero. That evidence supersedes the visual-density verdict above. The archive/material fixes and preservation checks remain verified; the requested density and hero revision require their own fresh captures and independent full finish review.
