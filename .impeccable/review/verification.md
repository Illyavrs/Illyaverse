# Serendipity build evidence

Code-first, no approved image comp. User pinned osu! layout inspiration, pure-black default, dark glass, light/dark toggle, rounded lettering and player personalization; no AI artwork.

Inspection round 1: desktop1440×1000 and mobile390×844;10 captures covering landing/settings/maps/chat/connect. Material fixes were batched: mobile community copy compression, primary Link-button foreground contrast, account theme reload race, translated sheet close and save description.

Inspection round 2:18 full-page captures, same viewports, with registration, docs, own profile and administrator added. Every capture was opened and validated. Browser capture reported0 page errors and0 horizontal document overflows. Markdown docs are long; inspect at original detail if needed. UI screenshot counts/messages are actual disposable local QA fixtures, not a fabricated production community.

Functional Chromium tests:3 passed on the real Docker stack. They cover black default, next-intl cookie/account locale, mobile navigation, registration, background upload, persistent theme, profile bio, chat, social add/block/unblock, logout and server RBAC. No trace recording or credentials in captures.

Static verification: TypeScript and ESLint pass. Production Next.js build passes in Linux Docker and Windows native. A project-local SWC cache retains the native loader's trust/hash checks; no system ACL changes. Runtime dependency npm audit reports0 vulnerabilities; current development eslint transitive dependencies report5 high advisories with no patched braces release available at inspection time.

Detector ran once after round2 on apps/serendipity/src and returned[]. No further visual polishing by the build thread; material changes must come from the independent finish reviewer.

Post-capture functional changes: SWR child caches are keyed by authenticated account to clear private caches on session changes; documentation filesystem tracing is bounded by explicit Docker copying; Windows native build cache moved to the monorepo root to keep materialized DLL paths shorter. All three Chromium flows passed again against the final Docker images.

The independent reviewer returned `fix` for two mobile compositions. One correction batch added readable minimum-width Markdown tables in labeled keyboard-focusable horizontal scroll regions, and stacked the profile score heading above intact game-mode controls. The same 18 capture paths were regenerated and each opened for validity; browser capture again reported zero page errors and zero document overflows. The same reviewer scored both fixes `resolved` and returned `ship` at that two-fix scope. No second detector was run.

The harness exposes generic subagents rather than the named shipped review role. Review used a fresh generic substitute, without inherited build history, instructed to load the shipped reviewer definition. The subsequent documenter follows the same fresh-context substitution. This does not establish native client compatibility.

Known disposable browser fixture users and messages were removed by exact IDs, usernames and test emails before the verdict captures. The preview now contains only the real bootstrap administrator and no beatmaps or scores; its activity strip reports those actual values.

## User-directed appearance correction, 2026-10-07

The user rejected the earlier promotional composition, poetic copy and thick plum glass. The previous visual verdict is not approval for that direction. The corrected surface uses direct labels in zh-TW/en/ja, neutral black glass on a pure-black default canvas, a compact community introduction and visible settings links. Defaults are 8px panel blur and 55% opacity. Background blur/dim remain separate; guests save appearance in the browser and authenticated players can save to their account.

The final correction batch produced nine valid local screenshots at desktop 1440px, mobile 390px and the actual 1254px in-app viewport, including English mobile navigation, Japanese settings, light/dark settings and synthetic uploaded-background customization. Every final screenshot was opened. A malformed initial QA background fixture was replaced with an original deterministic 640×360 PNG; it is test material, not shipped artwork. The default contains no background image.

Synthetic Playwright checks passed guest defaults, labeled slider bounds and zero values, browser persistence/reload, settings URL tabs and unknown-tab fallback, and an intercepted authenticated preferences PATCH/reload. No real account or database mutation was used by this browser batch. [Behavior evidence](appearance-qa.json) records the checks. These checks do not establish the entire website's functional completeness.

The detector ran once on the changed web targets: zero primary findings and 87 advisories relative to the superseded design documentation. A fresh independent reviewer opened all nine captures and returned `fix` only for documentation persistence; the rendered homepage/navigation/material/settings matched the requested direction. See [the appearance review](appearance-verdict.md) for the final disposition and exact scope. The named shipped roles were unavailable, so fresh generic reviewer/documenter agents followed the corresponding contracts.

The documenter updated token-bearing DESIGN.md, its parsed schema-2 sidecar and PRODUCT.md. The same reviewer scored the one persistence fix `resolved` and returned `ship` at that documentation-fix scope. No UI code changed after the captures and no second detector ran.

The combined source gate passed 157 Rust unit/CLI tests, 26 PostgreSQL integration checks and two existing native-artifact vectors, with formatting and all-target Clippy warnings denied. The final backend Docker images passed 308 isolated HTTP requests in 34 categories. Next.js production compilation, TypeScript and scoped ESLint passed. Both local Docker stacks were updated without inserting players, maps or scores; existing player counts remained one on localhost and two on illya.test. HTTPS startup probes passed using the retained Caddy CA. Actual stable/lazer login, gameplay and exit remain human-operated verification.

## Community workflow extension, 2026-10-07

Commit `994879b` completed profile avatar/banner removal, persisted map-board labels, valid native-mode controls and last-page moderation refresh. Six desktop/mobile captures were inspected, one detector run returned no findings, and a fresh generic finish reviewer returned `ship` for this incumbent extension. The named review/documenter roles were unavailable; fresh generic substitutes followed their contracts. The documenter retained the canonical DESIGN.md, sidecar and PRODUCT.md. See [workflow verdict](community-workflows-verdict.md), [documentation disposition](community-workflows-documentation.md) and [behavior evidence](community-workflows-qa.json).

The full fresh-database Chromium gate passed 40 cases with one existing opt-in capture skip; a focused nine-case catalogue run verified the final document-top captures. Source and final-image checks passed as recorded in [the community checkpoint](../../docs/verification.md#community-workflow-checkpoint). Both local stacks deploy Serendipity image `sha256:000aa230ee46ab7c122c805d4cfefe1535c4384f520af3791e65971ca259a98c`. Pure-black defaults, thin glass and user appearance controls remain the accepted direction; this scoped extension does not imply whole-product completeness.

The subsequent native map resolver is a backend-only change, with 160 unit/CLI checks, 33 isolated PostgreSQL checks plus two existing native vectors, and 310 final-image HTTP requests. It did not trigger a new visual design cycle or modify the inspected website components. Both stacks are healthy with the tested backend images; current native evidence and human-operated limits are in [the discovery checkpoint](../../docs/verification.md#native-map-discovery-checkpoint).

The later stable upload and lazer discovery follow-up remains backend-only. It passed 175 unit/CLI checks, 40 PostgreSQL checks plus two existing native vectors and 312 final-image HTTP requests, and both local stacks run the tested Resonance image. Actual stable POST413 was observed, with corrected attachment parsing still awaiting a human persistence check. This work did not alter the inspected interface or start a new visual cycle; current evidence is in [the native upload checkpoint](../../docs/verification.md#native-upload-and-lazer-discovery-checkpoint).
