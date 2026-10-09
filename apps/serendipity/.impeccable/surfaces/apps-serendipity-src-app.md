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

## October 9 correction

The user's supplied official profile score screenshot is evidence for compact, cover-backed records with visible grade, star difficulty, map/version/date, mods, aligned accuracy/combo/hit counts and right-aligned PP. Build an original composition and preserve real data, visibility and actions. Do not copy the upstream palette, source or imagery into assets.

Use genuine shadcn/Radix Select and DropdownMenu primitives throughout. All floating controls share the material system. Glass coverage is independent of blur/opacity: 0 keeps navigation and floating controls glass; identity joins at 25, section surfaces at 50 and content records at 75. Each excluded surface uses a deliberate solid counterpart rather than transparent content over arbitrary backgrounds.

Profile identity joins the banner with half of the avatar overlapping it. The section rail belongs to the joined masthead, not another card. The user explicitly requests rank-dependent gradient text, overriding the generic craft-floor gradient-text refusal for rank values only; use original colors and actual validated ranks, with honest unranked states. Charts focus on the dates actually observed while exposing the requested range and preserving missing/unranked gaps. Review desktop 1254×714 and mobile 390/320, plain dark/light plus uploaded-background glass levels 0/50/100, open selects, keyboard menus and long/partial data.

## October 9 map browsing correction

The two new user screenshots reject the opaque catalogue toolbar under full coverage and the homepage record's black hover strip. Preserve the approved world, replacing the toolbar's fixed 92% reading scrim with one actual section glass layer. Supporting copy and input placeholders remain readable; fields use thin neutral veils instead of accumulating another glass fill. A lower coverage tier uses its solid counterpart. Blur, panel opacity and scene dim remain independent.

Catalogue cards use bounded title/artist/mapper identity, stable preview tracks and aligned footers so unequal numbers of difficulties do not create unequal cards. Read osu-web's current card structure as abstract reference, while retaining original geometry, palette and code. Keep complete difficulty links, accessible touch targets, long-name bounds and filter/return history. Detail views keep full-set download beside identity and current difficulty, followed by useful selection and score records.

Normal map download is a complete `.osz` package. Prioritize a retained original archive through a strict archive-only route; an official set without a retained package uses a clearly identified full-package provider and an official-site fallback. A standalone local `.osu` has an honest unavailable-package state and cannot be presented as a playable full set. Never synthesize a misleading archive from a difficulty file or use existing public app credentials as if they grant full official downloads.

Homepage map hover belongs to the joined record: full-width restrained neutral feedback, unchanged geometry, visible keyboard focus and one blur on the containing material. Review the supplied desktop viewport, mobile390/320, dark/light, real empty/loading/errors, custom-background glass0/50/100, open Select, equal-card geometry and actual `.osz` actions. Synthetic artwork is QA only; no AI or new shipping raster.

Phone catalogue previews use one compact scrolling row with all eight difficulty links retained, instead of reserving several empty rows in low-difficulty sets. Homepage rows retain a full-width hover area while cover and numeric content stay inset 20px on desktop and 16px on phones.

## October 9 dense interface and navigation correction

The latest user screenshot rejects the tall toolbar and the previously reviewed 323px catalogue tracks. It supersedes that density verdict. Use consistently compact search/filter geometry and equal 124px desktop /112px phone cover records, with a single accessible difficulty popover instead of reserved blank tracks. Preserve complete difficulty access, real full-set download behavior and filter/return history.

The user supplied a transparent character PNG for the homepage hero, explicitly authorizing this artwork above the unchanged plain black default canvas. Copy its original bytes, document provenance, keep the head visible and separate the figure from text/actions at narrow widths. No AI image editing or generation.

Add purposeful Motion navigation selection and native document View Transitions with reduced-motion and unsupported-browser fallbacks. A slim top loading indicator follows real internal navigation and catalogue requests; success, failure and cancellation release it. Theme selection belongs to the existing Appearance menu, so remove the duplicate standalone theme icon. Dropdowns retain focus/dismiss behavior while keeping the page scrollbar/gutter and layout anchored; modal Select and Sheet compensation must not introduce a second gutter.

Primary user evidence: codex-clipboard-124584f3-341a-4eac-a5ea-c93691dabd7e.png (rejected toolbar), codex-clipboard-d2f8a098-281c-4ab3-b865-b8b8b4220dc7.png and codex-clipboard-7239a95c-c576-4db4-843e-d4a06cf784ae.png (compact reference records), codex-clipboard-1597b07e-6066-403e-b815-648b59f57d4d.png (supplied hero). They live in C:/Users/owenl/AppData/Local/Temp. Build fully, perform one batched desktop/mobile inspection and at most one consolidated confirmation, then obtain a fresh independent full finish review for this correction.
