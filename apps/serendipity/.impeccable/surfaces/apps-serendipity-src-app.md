---
version: 1
slug: "apps-serendipity-src-app"
primary_target: "apps/serendipity/src/app"
related_targets: ["apps/serendipity/src/features/home.tsx","apps/serendipity/src/features/players.tsx","apps/serendipity/src/features/beatmaps.tsx","apps/serendipity/src/features/map-discovery.tsx"]
---

# Community website redesign

THESIS: A coherent osu! community website makes players, beatmaps and scores recognizable through joined identity, aligned facts, distinct navigation and useful actions.

OWN-WORLD: Pure black default canvas; adjustable thin neutral glass with distinct heading, toolbar, record and inset surfaces; rounded Nunito/Noto typography; user accent for location, selection and performance emphasis; semantic grade/status/difficulty colors. No AI imagery, decorative background or slogans. Real covers and user banners remain runtime content.

STORY: Enter a factual community home, browse real recent maps and scores, choose a map difficulty with its facts and download nearby, or inspect a player's identity, rank history and score records. Registration, connection guidance, news, chat and permission-based tools remain discoverable.

FIRST VIEWPORT: A compact persistent identity/utility header and clear route navigation frame a 1200px content area. Home offers a short site introduction and primary actions above live counts, with genuine map and community content immediately below. Profile shows compact identity, mode/rank facts and the start of its content without an empty banner. Map catalogue shows search, mode/filter controls and scannable cover records; detail joins real cover/title/difficulty/actions beside compact factual statistics. Signature interaction: accent underline selections and contextual sticky section navigation retain orientation while browsing real data.

FORM: User-pinned osu-web/Sunset community-data canon; original implementation based on abstract source composition, not copied source, palette, imagery or exact geometry. Code-led build, no mockups. Replace the rejected flat composition rather than treating earlier DESIGN.md as approval. Preserve zh-TW/en/ja, light/dark, independent user background blur/dim and panel blur/opacity/radius, profile-public accent, accessibility, real empty/loading/error/retry states, routing/URL history, permissions and API truth. Target desktop1440, current1254, mobile390 and320. Build then one batched inspection, consolidated corrections and at most one builder confirmation; fresh independent finish review follows.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
