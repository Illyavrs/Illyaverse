# Map discovery and shared hierarchy review

Recorded 2026-10-08. This checkpoint covers the map catalogue, beatmap detail selector and the user-approved expansion of shared visual hierarchy. It preserves the existing plain black dark ground, adjustable neutral glass, rounded Nunito/Noto typography, direct product labels and personal appearance controls. Sunrise and osu-web were the user-selected layout references; this is an extension of the incumbent system.

## Source and image boundary

The final tested application images are:

| Service | Image |
| --- | --- |
| Serendipity | `sha256:85a70493fc24a9f33f59b2b4cafbb58ca033e096fdc0a936bf7f76a836b980ea` |
| Sanctuary | `sha256:f68711b230dde3990148a35978b3199d00e7ddfa0e14a706dd5b45c889291021` |
| Resonance | `sha256:8fac03733eb6713d2e322d07fc8e0431316a900d4adf7bc437afc19599092b0d` |

The [evidence packet](../.impeccable/review/map-discovery-evidence.json) binds these images to the source SHA-256 values, each required PNG and its metadata/hash. It also identifies the PostgreSQL and Caddy image pins. The final confirmation used the same exact application pins for the catalogue/package/score/profile browser gate and the appearance/rank regression gate.

Map discovery displays actual server-paginated sets, matching/stored difficulty counts, URL-preserved search/filter/sort/view/page state and direct difficulty links. The detail selector keeps the current version, compact quick links and pagination visible; a keyboard-accessible native disclosure opens fuller difficulty rows. The consolidated correction removed the duplicate expanded list from the default view and top-aligned catalogue cards. The desktop detail fixture's document height fell from 3357px to 1675px while keeping the full information available.

Shared hierarchy now distinguishes base content glass, raised filters/table headers, stronger persistent navigation, stronger heading dividers and live accent selection. Profile selection derives from the profile owner's public accent. Six numeric difficulty tiers have paired dark/light foregrounds and retain mode/version text and actual star ratings. The 8px blur, 55% panel opacity and 14px radius defaults remain independent live settings. `surface-subtle` is declared but currently has no component consumer.

## Verification

| Gate | Final evidence |
| --- | --- |
| Sanctuary unit cases | 75 passed; 98 opt-in cases ignored in that unit run |
| Isolated PostgreSQL suites | 8 catalogue, 6 availability, 13 package and 1 native-map cases passed |
| Rust formatting and Clippy | Passed |
| Frontend lint, TypeScript and production build | Passed after the consolidated correction |
| Catalogue and legacy browser gate | 75/75 passed, including 13 map-discovery cases |
| Appearance and rank regression | 17/17 passed: 5 appearance and 12 rank cases, on the same image pins |
| Documentation navigation | 10/10 passed; 28 registered guides |
| Retained-stack read-only runtime | 38 GET assertions passed: 18 main and 20 client-test; 10 healthy services on the exact final pins |

The first browser round passed 72 of 74 cases; its two failures were incorrect exact-count/cached-RX fixture assertions. Those assertions were corrected and the final gate passed. The profile capture fixture now returns a valid empty rank-history response instead of showing a misleading fixture 404. Synthetic fixtures intercept API calls; they do not change real accounts, fabricate retained activity or certify native client behavior.

One combined design detector ran before the consolidated correction: zero primary findings and 103 advisories (6 color, 73 font-size and 24 radius). The six color advisories identify incumbent mode-art values (`#332131`, `#332c21`, `#1e302b`, `#a6d8c6`, `#282338` and `#c3b8eb`); the approved difficulty foreground roles are separately recorded in the design tokens. The cycle remained bounded to one batched first inspection, one consolidated correction and one confirmation; there was no second detector or further polish loop. Detailed build/source/browser logs remain under ignored `data-map-discovery/`. Owned disposable fixture resources were identity-checked and removed; old artifacts and retained stacks were preserved.

## Capture evidence and provenance

The root inspected all ten final full-page PNGs once. The packet and individual metadata record loaded fonts, the expected locale/theme, zero scroll at document top and document width equal to viewport width. These are synthetic review states, including a valid empty profile rank history.

| Final capture | Viewport | Theme / locale / state |
| --- | --- | --- |
| [Catalogue grid](../.impeccable/review/map-discovery-desktop-dark-zh.png) | 1440 × 1000 | Dark / zh-TW |
| [Catalogue at actual browser width](../.impeccable/review/map-discovery-1280-dark-zh.png) | 1280 × 720 | Dark / zh-TW |
| [Catalogue list](../.impeccable/review/map-discovery-desktop-dark-zh-list.png) | 1440 × 1000 | Dark / zh-TW |
| [Beatmap detail](../.impeccable/review/map-discovery-detail-dark-zh.png) | 1440 × 1000 | Dark / zh-TW / compact selector |
| [Catalogue mobile](../.impeccable/review/map-discovery-mobile-light-en.png) | 390 × 844 | Light / en |
| [Catalogue narrow mobile](../.impeccable/review/map-discovery-mobile-dark-ja-320.png) | 320 × 844 | Dark / ja |
| [Landing populated](../.impeccable/review/map-hierarchy-home-1440-dark-zh-TW-populated.png) | 1440 × 1000 | Dark / zh-TW / public synthetic collections |
| [Landing recovery](../.impeccable/review/map-hierarchy-home-390-light-en-error-empty.png) | 390 × 844 | Light / en / error and empty states |
| [Profile with banner](../.impeccable/review/map-hierarchy-profile-desktop-1440-dark-zh-banner.png) | 1440 × 1000 | Dark / zh-TW / owner accent |
| [Profile without banner](../.impeccable/review/map-hierarchy-profile-mobile-light-en-no-banner.png) | 390 × 844 | Light / en / owner accent |

No new shipping raster or AI artwork was introduced. Official covers remain browser-rendered runtime content from `assets.ppy.sh`, using validated positive official set IDs below 2,000,000,000. Local maps and failed cover loads use the existing library SVG fallback. The review PNGs are gitignored screenshots, with their provenance recorded in the evidence packet; they are not product artwork. The default dark background remains plain black.

## Deployment and read-only runtime

Both retained local stacks now run the final application image pins above. Backups use `before-map-discovery-20261008T041659Z-{main,client}`. All ten services are healthy; the four PostgreSQL/Caddy container identities and both unchanged Resonance container identities were preserved. Schema remains at 22, with schema/checksum, user/map/score/replay, announcement/community, availability, historic-rank fingerprints and prior rank batch keys preserved. No host, trust, CA or firewall setting changed; no native client was launched and no real account was mutated.

| Retained data | Main | Client-test |
| --- | --- | --- |
| Users / unrestricted users | 2 / 2 | 2 / 2 |
| Maps | 0 | 44 |
| Scores / replays | 0 / 0 | 0 / 0 |
| Rank rows / partitions | 14 / 7 | 14 / 7 |
| Last rank capture (UTC) | 2026-10-08 04:08:44.847297 | 2026-10-08 04:09:05.088758 |

Read-only runtime verification passed 38 GET assertions, split 18 main and 20 client-test. Main has no active stored set, and neither stack has an actual hidden set; those runtime cases were skipped without adding fixtures or changing moderation. Their isolated PostgreSQL coverage remains separate evidence. Native stable/lazer sessions and received local room receipts were all zero. Initial helper-only failures involved the page clamp (maximum 1,000,000) and canonical website route `/beatmaps`; failed logs were retained, and the corrections required no application-source or image change. Detailed runtime evidence is in ignored `data-map-discovery/runtime.log` and the evidence packet.

A separate read-only check in the actual 1280 × 720 in-app browser confirmed `/beatmaps` with 39 sets and 24 results on its first page, plain black ground and no document overflow. The first eight visible official covers loaded; remaining offscreen covers retained lazy loading. Actual `/beatmaps/44` displayed three known siblings, official Ranked status and a truthful `.osu`-only download without claiming an archive. This was a runtime inspection, not another screenshot or polish pass.

## Documentation, verdict and remaining scope

A fresh generic agent substituted for the unavailable dedicated `impeccable_documenter` role. It read the shipped `agents/impeccable_documenter.toml`, degraded documenter reference and document-format reference in full, then updated [DESIGN.md](../DESIGN.md) and [.impeccable/design.json](../.impeccable/design.json) within the approved role expansion. All 24 approved dark/light surface, divider, selection and difficulty primitives match the CSS; all 61 color entries have metadata. The sidecar remains schema version 2 and does not duplicate the frontmatter's primitive-token catalogue.

Unrelated incumbent drift was reported and preserved: the active landing layout differs from the legacy hero height/line-height guidance; the accent provider's contrast calculation differs from the recorded 0.179 threshold; native-control font inheritance and universal border-color overrides retain their existing cascade behavior. Those discrepancies were not repaired or promoted into new rules by this checkpoint.

The [fresh finish verdict](../.impeccable/review/map-discovery-verdict.md) is **ship** for this bounded incumbent extension, with no material fixes. A fresh generic reviewer substituted for the unavailable dedicated `impeccable_finish_reviewer`, read its exact shipped role and reviewed all ten final captures plus the supplied source/image packet. The five-section contract records persistence, fidelity, ceiling, material fixes and what to keep; TYPE, MATERIAL and GROUND match the approved direction. The map discovery/shared hierarchy disposition and subsequent retained-stack deployment/read-only runtime checkpoint are complete within their stated boundaries.

RBAC, withdrawal, actual `.osz`/`.osu` file-kind labels, download/replay authority and Vanilla/RX behavior remain within the existing capability boundary. The original server goal remains active. Real stock stable/lazer gameplay and successful persisted score/replay submission are still **pending user-operated client verification**; source, database and browser fixtures do not establish those outcomes. See [map discovery](map-discovery.md) and the broader [verification record](verification.md).
