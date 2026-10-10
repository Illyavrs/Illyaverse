# Profile records and appearance coverage

This October 9 refinement addresses the player's screenshot and feedback about inconsistent controls, excessive glass, the profile banner/avatar connection, rank colors, chart presentation and score records. It extends the October 8 community layout without copying upstream source or artwork.

## Implemented behavior

- Appearance settings expose **glass coverage** separately from panel blur, panel opacity, corner radius, background blur, background dim and accent. Preferences persist for guests and signed-in accounts. Older account preferences receive the default of 100.
- Navigation and floating controls always retain glass. Identity surfaces join at 25, section surfaces at 50 and content surfaces at 75. Each group has coordinated solid counterparts in both themes. Panel blur remains 8px, opacity 55% and radius 14px by default; the dark canvas remains pure black.
- Visible selection fields use the shared shadcn/Radix Select primitives. Score actions use DropdownMenu. Portalled popups, dialogs, sheets and fields share material variables, keyboard navigation and focus behavior.
- The profile avatar overlaps halfway into an uploaded banner. Profiles without banners reserve no empty artwork region. Mode, rank and section navigation join the identity composition instead of becoming isolated cards.
- Validated profile ranks use an original placement-dependent color gradient, as explicitly requested. Missing placements remain ordinary unavailable values. The public profile accent stays independent of the visitor's private appearance.
- The ranking chart shows actual observed dates, missing or unranked breaks and selected daily facts. A daily table supports touch and keyboard use. Focus instructions do not shift the chart or disclosure controls.
- Best and recent score rows combine runtime map covers, recorded grade, stars, map identity, mods, accuracy/combo/hits, PP and accessible score/replay actions. Best-score contribution and original score remain distinct. API metadata comes from persisted native score records; a passed grade is never guessed from accuracy.
- Desktop score rows use a 46px minimum and 3px gaps while retaining 44px action targets. Mobile gives titles a dedicated two-line area at 13px, with 11–12px supporting facts below instead of squeezing identity beside PP.
- Catalogue heading and filters share a theme-aware reading ground over uploaded backgrounds. It adds no extra backdrop blur and does not change coverage, opacity or background dim; worst-case supporting-text contrast is 5.04:1 in light mode and 8.31:1 in dark mode.

The API change is additive: score responses include artist, official beatmapset ID, difficulty stars, map maximum combo and normalized recorded grade. Existing privacy, publication status, ranking, contribution and Replay access rules remain in force. No SQL migration or protocol ingestion change is required.

## Evidence and scope

The primary visual reference is the player's attached score-list screenshot. The source studies and pinned osu-web/Sunset revisions are recorded in website-redesign.md. Original design rules are recorded in [DESIGN.md](../DESIGN.md) and the Serendipity surface brief.

All writable verification uses owned disposable PostgreSQL/Compose environments. The browser captures use explicitly synthetic activity fixtures; gray banner/avatar and a small geometric uploaded-background fixture are original non-AI QA pixels. They are not shipping assets or production activity. Actual official map covers remain runtime content, and personal images remain user uploads.

Dedicated finish-reviewer and documenter roles were unavailable. Fresh independent agents followed the corresponding degraded-role instructions. The full finish review opened all 22 authoritative captures and the original player screenshot. It requested two material corrections: denser desktop/readable mobile score composition, and readable catalogue heading/filter text over uploaded backgrounds. A verdict-only follow-up found both resolved, no related regressions and matching capture hashes/dimensions; its disposition was **ship**. The documenter separately rechecked durable rules, schema 2 and ten self-contained component previews.

One detector pass reported the explicitly requested rank-text gradient as its sole primary finding, plus 174 advisory differences from the prior token scale. The gradient exception is limited to validated profile rank values. The detector was not rerun to chase inherited advisories.

## Validation record

Before the finish-review corrections, the production Web image passed lint, TypeScript and all 203 owned browser tests. Additional bounded capture checks passed 8 profile-layout and 14 catalogue tests. Sanctuary passed the two score-grade unit tests and five owned PostgreSQL integration tests; domain preference tests passed 32 with one pre-existing ignored test.

The corrected Web image `sha256:6a5821ba1929b421f0a55db3dd6c5c2017fa2d9f55a2d43ee44720d53d92c4b8` passed full lint, TypeScript, production build and all 203 browser tests (4.5 minutes) in `illya-web-smoke-c363781af86273d3`. The harness verified removal of its containers, network, volumes and temporary environment; independent read-only inventories were empty. All 22 authoritative screenshots were refreshed against this image.

The API image is `sha256:ac310e405e5dcd69b1b07f5e9ff0b9b4320a5e26bd1b9c5b1fa4e2cbe8c05846`, validated in the owned PostgreSQL integration run. Local proof files are retained under `data-profile-material-20261009/`.

## Local deployment

Both local stacks updated only Sanctuary and Serendipity to the tested images. All ten services are healthy. The six protocol/database/proxy container identities and start timestamps were unchanged at this deployment gate. Both databases remain at schema 22; every previous user, map, score and replay-score ID was retained. Client-test retains two users, 48 maps, two scores and one Replay; the main stack retains two users.

The guarded script creates no database dump, restore, migration, image build or pull. It validates existing backup hashes and records previous image pins and manual rollback instructions. A preliminary read-only preflight failed on PowerShell's automatic ISO timestamp conversion before any recreation; preserving JSON dates as strings fixed that guard, with 31 offline checks passing. The final deployment passed under `data-profile-material-20261009/deploy-20261008T173646Z-e2def650e38e408e8b2ba3df347e2427/deployment-manifest.json`.

Read-only checks of the deployed client-test API confirm score 2 retains 19.782516 pp, its recorded SS grade, 1.769817 stars and map maximum combo 87. Replay download returns HTTP 200 and 13,047 bytes. Fresh browser inspection confirmed the actual home, player 2, catalogue's open shadcn status menu, map 42 and appearance settings. The latter expose coverage 100, panel blur 8px, opacity 55% and radius 14px. No production preferences or records were changed during inspection.

The old preview tab initially reported `ERR_CERT_DATE_INVALID`. Read-only TLS diagnosis showed the served/stored leaf matched and remained valid from `2026-10-08T13:31:27Z` to `2026-10-09T01:31:27Z`, with a matching existing trusted root. Caddy had renewed automatically; normal Windows HTTPS validation returned 200. A fresh browser tab then opened the site normally. The stale-tab/session explanation is an inference, not a diagnosed browser bug. No certificate warning was bypassed, and no proxy restart, trust, hosts, firewall or configuration change was needed.

This document does not claim completion of the broader native-client project.
