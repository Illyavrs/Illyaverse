# Profile Best/Recent documentation

Generic documenter substituted under `reference/degraded/documenter.md`; `reference/document.md` was read in full and retained. This fresh source pass records an ordinary Operate extension. `DESIGN.md` and `.impeccable/design.json` remain unchanged; there is no new world, seed, comp, artwork, primitive token or global rule.

## Evidence and retained system

Checked `PRODUCT.md`, `DESIGN.md`, the sidecar, `.impeccable/surfaces/profile-best.md`, `docs/profile-best-performances.md`, `features/players.tsx`, `features/scores.tsx`, `lib/types.ts`, `app/globals.css`, and the relevant profile/status copy in zh-TW/en/ja. This record describes source and the documented API contract; it makes no browser, test, build, deployment or native-gameplay claim.

The profile retains pure-black default ground, neutral translucent glass, the paired light theme and rounded Nunito Variable/Noto Sans TC typography. Existing panel blur (8px), opacity (55%) and radius (14px) remain independently adjustable from background blur and dim. The incumbent profile accent scope binds its accent, contrasting foreground and readable-accent focus treatment through semantic variables. Banner and avatar content still come from player media. The extension adds no background or generated image. [Source](../../apps/serendipity/src/app/globals.css#L4), [profile scope](../../apps/serendipity/src/app/globals.css#L1380)

Existing **Personal Accent**, **Paired Theme**, **Bilingual Breathing**, **Local Overflow**, **Glass Before Shadow**, **Independent Material Controls** and **Shared Radius** rules remain the system authority. No local collection behavior is promoted into a new system rule.

## Reused local patterns

The banner, identity, social actions, mode/Normal–RX filter and statistics retain their existing hierarchy. The score section reuses the glass content panel, secondary partition badge and shared section heading. Its heading and badge wrap together with a 12px gap. Best/Recent uses the incumbent bordered mode-control track and native pressed-state buttons, with an accessible group label and `aria-pressed`. At 600px the header stacks, the track expands to full width and its two buttons share the width with intact labels. [Profile source](../../apps/serendipity/src/features/players.tsx#L189), [local styling](../../apps/serendipity/src/app/globals.css#L1648)

The active collection has one factual description using the existing 12px hint type, secondary text, 1.85 line-height and 65ch maximum measure. Local spacing places it between heading and table. Common body type remains 15px/1.7, reduced to 14px on small screens; no new type role is introduced. Empty states name the collection, and the active heading/description remain present around the reused Loading and LoadError/retry components.

Best reuses `ScoreTable` with added position, pp, weight and weighted-pp columns. Values come from `position`, `pp_weight` and `weighted_pp`; the browser formats pp/contribution to two decimals and weight as a two-decimal percentage without calculating rank or weighting from the page. Missing fields remain a dash. The weight is secondary text, pp retains the readable accent, and dense numeric strings use tabular figures and remain intact. The Best wrapper is a labeled, keyboard-focusable locally scrolling region. Original score, result link, date, mods, status and replay action retain their existing presentation. [Table source](../../apps/serendipity/src/features/scores.tsx#L9), [response types](../../apps/serendipity/src/lib/types.ts#L9)

## Collection and navigation behavior

The documented Best contract is strictly public, server-selected qualifying performances ordered by pp; the frontend requests `/users/{id}/scores/best`. Recent retains `/scores` with an account-scoped cache key, disables reads while session identity loads, uses no previous-selection data, and purges the precise failed key after 401/403/404. These are observed request/cache boundaries, not independent proof of server policy. [Source](../../apps/serendipity/src/features/players.tsx#L213)

Best is the default. Live URL merges preserve mode, variant, collection, page and unrelated URL context; collection/partition changes reset page one. History uses pushState for selection and replaceState for clamping. The clamp checks the live pathname and selection before applying a loaded collection's last page. Mania normalizes to Normal. All three locales explain eligibility, server-supplied contribution and Recent visibility directly, without slogans or unsupported compatibility claims. [Selection source](../../apps/serendipity/src/features/players.tsx#L147)

## Drift not canonized or repaired

The incumbent global `font: inherit` and universal border-color cascade remain the caveats already recorded in `DESIGN.md`; they are not new control hierarchy or invalid-state rules. The preserved sidecar uses raw user-accent outlines in previews while source focus uses the readable `--ring`/profile readable accent. This existing preview mismatch is reported without repair or new normative values. Global polish and system reconciliation are outside this extension's authorization.
