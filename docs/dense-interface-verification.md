# Dense interface refinement — 2026-10-09

Status: shipped to the retained client-test and main websites. The full scoped finish review returned `ship`; a subsequent narrow scroll-lock compatibility fix passed independent scoring, 26 affected browser tests, current-source lint/nonincremental TypeScript/build and actual retained-browser verification. Final Web image: `sha256:d1309dd7d4e405afa0a09ade2955fca5146152f34a13ab0e78ae96f559ddd4be`. This record covers the interface refinement, not completion of the whole game server.

The user's newer catalogue screenshots reject the previous 323px density and supersede the narrow visual-density verdict in `docs/map-card-correction-verification.md`. This refinement records the approved compact toolbar/cards, supplied homepage character, purposeful navigation motion, top loading feedback, one Appearance control and action-menu scroll behavior. Previous archive behavior and retained-data verification remain separate evidence; they do not verify this new interface.

## Current source

### Home

- The home container remains 1200px wide at most. Its desktop padding is 24px 32px 40px, reducing to 16px 16px 32px at 600px.
- The introduction has a 256px desktop minimum height, text/actions beside a bounded character region, and a joined capability band. The supplied character is cropped and scaled with CSS; it is separate from the page background. The default dark canvas remains plain black.
- The homepage uses `/images/home-character.png` through an unoptimized Next.js Image, with empty decorative alternative text, `aria-hidden` on its container, eager loading, and high fetch priority. The container does not receive pointer events.
- The hero image enters once over 300ms with a small vertical settle. A reduced-motion rule disables this animation. At 600px the character region is 144px square; at 360px it is 124px square. Mobile copy, description and actions retain separate layout regions.
- Home record headings use 12px 20px padding. Map rows have a 104px minimum height, 72px desktop covers and 12px gaps. At 600px their padding is 8px 16px, covers are 56px × 72px, and the 104px minimum remains. The compact-record selectors explicitly outrank later shared map-row rules for geometry and hover/focus. Row hover/focus uses a thin neutral veil without another backdrop filter.

### Catalogue

- One section material layer contains the heading, search and filters. The toolbar has 12px 16px padding, or 12px on phones. Its heading is 24px with line-height 1.25. Search and four filters share one row above 1050px; below that they stack. The filter grid has two columns at 600px.
- Search and action controls have a 44px desktop minimum and a 48px phone minimum. Fields inside the toolbar use a 4% neutral foreground veil with no additional backdrop filter; Select hover uses 8%.
- Grid and list cards share a 124px desktop height and a 112px height at 600px. Grid results use two columns with a 10px gap, becoming one column with an 8px gap at 780px. Cover width is 96px, then 82px at 600px and 70px at 420px.
- Each card has bounded single-line title, artist, mapper and fact tracks, with a 44px bottom status/difficulty band. Its main grid uses `minmax(0, 1fr)` for the row and zero minimum height for the copy. Long labels retain full identity in their title attributes and link content. A genuine official cover can appear as a faint backdrop (14% opacity); the missing-cover fallback preserves card geometry.
- A 44px difficulty-summary button shows up to eight colored preview ticks and the matching count. Its floating popover uses `min(340px, var(--radix-popover-content-available-width))` and the available height, with 12px collision padding. Focus is directed to the first difficulty link. The popover has a labelled 44px close button and retains each preview link's mode, version, numeric stars and complete accessible label. When there are further matches, a link leads to the complete set selector. Vertical overflow is local to this popover.
- The card summary replaces the superseded reserved difficulty tracks and phone horizontal preview rail. The map-detail complete set selector remains a separate surface with its existing local overflow behavior.

### Navigation, loading and appearance

- `motion` is declared as `^14.0.0`; the installed package reports version 14.0.0. `LazyMotion` loads the shared `domMax` features, and `MotionConfig` uses the visitor's reduced-motion preference. Separate desktop/mobile `LayoutGroup` scopes move a shared active underline with a spring (stiffness 450, damping 38).
- The utility header remains 60px tall. The route row is now 44px. A one-pixel route top seam and the header's one-pixel bottom border make the desktop shell 106px; the mobile shell is 61px. Header actions, locale selection, route links, action-menu options and sheet close controls have 44px minimum targets.
- `--site-header-height` supplies 106px desktop and 61px at 800px. Profile sticky navigation consumes that variable, and anchor clearance adds 65px (171px desktop, 126px mobile). At 600px the header uses an 8px gap, keeps the compact brand mark and hides the direct account profile link; mobile navigation still provides profile access.
- One Palette/Appearance button opens the shared Appearance sheet. Its Theme group retains dark, light and system choices. Accent, glass coverage, panel blur/opacity, radius and background controls remain in the same sheet. The source currently implements the theme choices as a segmented group inside the sheet.
- `NavigationLink` preserves Next.js anchors, prefetching and modifier-key behavior. Exact current-URL selections call `preventDefault` and remain no-ops; hash-only changes retain ordinary anchor behavior. It coordinates same-origin route changes through the navigation provider. Native View Transitions run only when supported, the page is visible and reduced motion is not requested. Only route content receives the 180ms transition; the persistent shell remains anchored. A slow native snapshot is released after 700ms while request feedback continues.
- A fixed 3px top bar reflects outstanding route work, initial SWR loads and explicitly tracked public-map requests. The SWR middleware observes `isLoading`, so cached background revalidation remains quiet. Request tokens are released on settlement and cleanup. The bar has a translated progress label; reduced motion uses a static full-width line.
- Action DropdownMenu roots default to `modal={false}`, leaving page scroll available. A stable native scrollbar gutter is reserved on the document. The `html body[data-scroll-locked]` override suppresses additional RemoveScroll margin and compensation, outranking the library's later important rule. Actual retained-browser Select and Appearance inspection confirmed zero horizontal displacement.

## Supplied asset

The authorized image is a 700 × 1251, 8-bit RGBA PNG (color type 6), 457,931 bytes. Direct file inspection confirms SHA-256 `d32cc7271ff33364e247c8d8988c16f61e50d7715cd500b6cbdece29c53c70e4`, matching the supplied original. Its bytes and alpha are preserved. The provenance file is `apps/serendipity/public/images/home-character.provenance.md`. No generated image is part of this refinement.

## Design record

`DESIGN.md` and `.impeccable/design.json` intentionally merge the approved density, authorized character, navigation and loading facts into the existing Community Record system. Existing palette/material primitives, original implementation constraints, Nunito/Noto pairing, three locale hierarchy and unrelated component rules are retained. The sidecar carries updated motion/breakpoint extensions, synchronized narrative and a self-contained compact catalogue preview alongside the ten incumbent previews.

This documentation pass used the degraded Impeccable documenter contract because the specialized role was unavailable. It inspected source and asset metadata only; it did not run a server, browser, render, test, build, deployment or commit.

## Source snapshot

The final implementation is frozen for this snapshot. The 36 SHA-256 entries below match `data-scroll-lock-compatibility-20261009/current-source.json`: runtime source, package manifests, the supplied image and its provenance, and the affected test sources. General documentation, design records, installed dependencies and this record are excluded. The original full visual packet retains its `31e6cc4c…` provenance; six navigation captures were refreshed on the final `d1309dd7…` image after the two-file compatibility fix. Source hashes alone establish neither a visual verdict nor deployment identity.

<!-- BEGIN:SOURCE_HASH_MANIFEST -->

Final snapshot recorded by the root: `2026-10-09T05:49:11.1064565+00:00`; base commit: `ceeac157d697e3f4d6427d8552fedd47b301aa5d`. All 36 entries match exact current workspace file bytes.

| Source | SHA-256 |
| --- | --- |
| `apps/serendipity/package.json` | `1728de929b52a1249881ef4c9c610dcdab35cfbd75acfec705e5fc98a2305fd4` |
| `apps/serendipity/public/images/home-character.png` | `d32cc7271ff33364e247c8d8988c16f61e50d7715cd500b6cbdece29c53c70e4` |
| `apps/serendipity/public/images/home-character.provenance.md` | `58e9cd5c8e2e9576c4fcd3404ccabc1946f46dbc5a4af8264cf5b610afbd3297` |
| `apps/serendipity/src/app/layout.tsx` | `747fa3b50704dadd372339843c0832330dd90126da6b88e0538bf99fa257dd70` |
| `apps/serendipity/src/components/common.tsx` | `36d0447d5850871074d704e097c80b2c38dc9de2e153bf27590250320c8b23df` |
| `apps/serendipity/src/components/motion-features.ts` | `411426ac31bae89fff3b9727bf6fda0d2fbe1258068d143ca6bd128bd56a01da` |
| `apps/serendipity/src/components/navigation-motion.tsx` | `943206f2dbd0bf055fd3c2dd0ac7fcb3eaa30d9b9237fe8f7bebb966e4a5cf8b` |
| `apps/serendipity/src/components/profile-score-list.tsx` | `1863f7d9abe170ee2aafcf097404895769309d5fab52a48c22a8457310c711a1` |
| `apps/serendipity/src/components/providers.tsx` | `fa4f6fa3f8a49761ebaf5550f39fed0906106cf4d99d37c5d004d2dc0c155761` |
| `apps/serendipity/src/components/shell.tsx` | `06e3308e3c6126b5ddb84bbb7b394b1b717ff670b06fb5609a62f899effcd733` |
| `apps/serendipity/src/components/ui/dropdown-menu.tsx` | `a1b0c48f0f206215f35c5f01c3dd9d1978bc6278ccbdbcc2204317e6eb9603f0` |
| `apps/serendipity/src/components/ui/sheet.tsx` | `3bed00c5e4e3d70ebac7e551e0d6faf4d1c8435b963ee48f6fbb8a3c2db6cad9` |
| `apps/serendipity/src/features/admin.tsx` | `4d2966a2d293f9d690c4bcbc45946280a84f819284e7676d59287901cefe893c` |
| `apps/serendipity/src/features/announcements.tsx` | `337a8bedb529b4208488fd79d73ed6ec38f2f55ffec10531bceb59a7aa642099` |
| `apps/serendipity/src/features/auth.tsx` | `0ffe7673ec07956370a3d48c43c75327aa7480f25a0da904e6ee78b4be5e06d0` |
| `apps/serendipity/src/features/beatmaps.tsx` | `8ca8477e66056000e65ce634a4b22f93ad3a56b74505ab28c744c0947c6d0c6e` |
| `apps/serendipity/src/features/chat.tsx` | `9aaec6efef148069a521c95ffab70561a716f4ef41ac884a69ba0bbd09f76991` |
| `apps/serendipity/src/features/connect.tsx` | `0844409fd990d4135ca9afeb1c9761baae375a65e7b734f8e5ee3af8b91c4555` |
| `apps/serendipity/src/features/home.css` | `4dd09eea200d55e14300d9209a182ce02cb5175fea6c9f0378e04bfc76ec9f65` |
| `apps/serendipity/src/features/home.tsx` | `5de64565393391f5f98373200f4f30e0ce5d00bb332c9a05af9db70c1c8f9131` |
| `apps/serendipity/src/features/map-discovery.tsx` | `cb56857ef6a4747dcc0d8b25301974985236dfe9b77cce73b6216cf1eee52d40` |
| `apps/serendipity/src/features/players.tsx` | `01a7ce5b0857d79c3c602050e2dfa031897ba2c95ed5bd6a6273d6352ff968f5` |
| `apps/serendipity/src/features/profile.css` | `d46c2aa08b346b47485eecbc472e2cefa814d01b6df312efb2b3f9cb1b7d61ff` |
| `apps/serendipity/src/features/recovery.tsx` | `e5e810587d1382429cfaaaec25c6f2931408c27bf25581a71160254a449caa73` |
| `apps/serendipity/src/features/score-detail.tsx` | `4c71c0c70142d944878a178f48a6f51aca5c36b2172e0c06533babc5e4a7b1da` |
| `apps/serendipity/src/features/scores.tsx` | `c0ca1e97c2460a52078ca1a63e90008452c98215060c2ca5fbf2fe3d4cf74228` |
| `apps/serendipity/src/features/social.tsx` | `814b220ad8d70faf1a0246381a55045714e4f188801f09de416b47e2e114afca` |
| `apps/serendipity/src/styles/map-discovery.css` | `039bb7df7987059e66f5bbd53e79afb6374d0b88c2febd537a3003998c592703` |
| `apps/serendipity/src/styles/navigation-motion.css` | `d432ab6bf7d6f6ced437ffe12e4a1e7d2af2da5bb2fa20866ba80c82bb57db9a` |
| `apps/serendipity/tests/appearance-state.spec.ts` | `5dcc6d2730d983417b5fd2107f94a2c96222d6b23f91102f46268308d1eb690a` |
| `apps/serendipity/tests/home-community.spec.ts` | `e6977033b0d58790d6171068febd766c04373a9a85db6df2c6286439ebe6b204` |
| `apps/serendipity/tests/map-discovery.spec.ts` | `87266a29fa2da64a624a2eb1eb75b0de220c0e5feb364e1f7ba375407701ace5` |
| `apps/serendipity/tests/navigation-motion.spec.ts` | `28bc148cb5de4a5cc867311100f794969b80d78071de3f742a1bf5dfd62ac257` |
| `apps/serendipity/tests/profile-layout.spec.ts` | `3bdb2e9fb74bbbaaeebf2b5e23cd86c4d916ec9c0787178d1bb753151d1abbb1` |
| `apps/serendipity/tests/rank-history.spec.ts` | `7a180c01707fc4df283a4aac5d1aaf496c93a5455f4a16131270541e304b3e93` |
| `package-lock.json` | `d5ab7c1bc4c060d5a20465d05860065dd6582b691fc2f1dcfe9561ca5c8fb287` |

<!-- END:SOURCE_HASH_MANIFEST -->

## Verification to complete

| Evidence | Status |
| --- | --- |
| Final source/hash snapshot | 36/36 current bytes match; all 282 source declarations in capture metadata match actual files |
| Frontend lint, TypeScript and production build | Passed on current source; production build recorded in `web-build-final.log` |
| Affected browser behavior gate | Full refinement image `31e6cc4c…`: 87 passed (2.1m); final compatibility image `d1309dd7…`: 26 passed (34.6s); both owned harnesses cleaned |
| Fresh desktop/phone, light/dark, locale and glass-coverage captures | Root confirmation complete; 40 metadata/PNG pairs verified; independent reviewer inspected all 20 required captures |
| Difficulty popover keyboard/focus access and long metadata | Passed final browser gate and fresh independent visual review |
| Header Appearance controls, dark/light/system selection and scrollbar movement | Passed final browser gate and fresh independent visual review |
| Route, request/error/retry and reduced-motion behavior | Passed final browser gate; initial SWR load/revalidation distinction also verified in source |
| Fresh independent full finish review against the user's latest screenshots | `disposition: ship`; no material fixes; recorded in `.impeccable/review/dense-finish-review.md` |
| Tested image identities and retained-data deployment proof | Final guarded Web-only deployment passed for both projects; protected eight services/storage/schema/data preserved |
| Actual retained-data browser inspection | Passed at `https://illya.test`: Select/Appearance horizontal displacement 0px, one Appearance entry, original hero loaded, black canvas, genuine activity and settled route feedback |

The authorized profile clearance correction uses the shared 106px/61px header-height variable. Desktop/phone clearance, catalogue metadata legibility and hero/copy separation passed the fresh capture review. Small catalogue metadata values remain surface-specific source facts, not additions to the shared type ramp.

## Detector record

The detector ran once and reported 90 findings: 89 advisories (72 type, 13 radius and four color) and one `gradient-text` warning at `apps/serendipity/src/features/profile.css:267`. The warning identifies the explicitly user-requested profile rank text gradient already recorded as a narrow exception in `DESIGN.md`; that exception remains limited to validated rank values and retains forced-colors/unranked fallbacks. It does not establish gradient text for other typography. The detector was not rerun by this documentation pass. Advisory counts do not establish legibility or replace fresh image inspection, the browser gate or independent finish review.

## Root verification evidence

The root's current-source frontend ESLint and nonincremental TypeScript checks passed. The production build in `data-dense-interface-20261009/web-build-final.log` completed and wrote tested Serendipity image `sha256:31e6cc4cff6294fbfec1a797951995ab7eb4bbd756d731907c394b10f8786c6f`.

The final isolated browser gate ran 87 tests in eight suites: `appearance-state`, `auth-identifier`, `community`, `home-community`, `map-discovery`, `navigation-motion`, `profile-layout` and `rank-history`. All 87 passed in 2.1 minutes on that exact image. `data-dense-interface-20261009/browser-gate-final.log` records the pinned image, individual results, and verified removal of the owned `illya-web-smoke-a87d5240b23fa6df` containers, networks, volumes and temporary environment. It covers compact cards across widths/views, popover keyboard/touch access, material coverage, homepage hover/focus, ordinary anchor behavior, route/request feedback, scrollbar/focus behavior, shared profile clearance and affected account/profile regressions. The capture cases use the new dense namespace; profile captures include bounded visual settling, and rank-focus checks avoid scrolling while measuring state.

An earlier broader run on image `2fa9f493d675f077fc63a6b854d82bc59641a5dd351011f0d69c3fc896901552` reported 234 passed, six failed and one skipped. The six failed cases were covered and resolved by the final 87-test gate. That earlier run is separate evidence; the complete 241-test collection was not rerun successfully on the final image.

The root completed its single consolidated confirmation pass after the first inspection's fixes. A separate read-only provenance audit verified 36 source hashes, all 282 source declarations and 40 metadata/PNG pairs; all six explicitly declared screenshot hashes match. The six regenerated legacy filter-popup pairs are byte-identical to their final dense counterparts and are retained as generated evidence. PNGs are local ignored capture artifacts; metadata and this verification record are tracked.

The fresh independent reviewer opened the four latest user references before reading the builder contract, then inspected all 20 required current desktop/phone, light/dark, popover, navigation and profile captures. Its full scoped verdict is `disposition: ship`, with no material fixes. The unavailable specialized reviewer was replaced by a fresh default agent following the degraded finish-reviewer contract; no QUALITY BAR card was supplied for this established pinned refinement. Optional dropdown pixels, sidecar preview bodies and unsampled source portions were explicitly excluded. Its findings are recorded in `.impeccable/review/dense-finish-review.md`.

### Retained-browser compatibility correction and final deployment

After deploying the first reviewed image, root CUA found that the real browser still narrowed the body/header/main by 15px when Source Select opened (1265px to 1250px). CSSOM showed Radix injecting an equal-specificity `body[data-scroll-locked]` rule with `margin-right: 15px !important` after the application stylesheet. This contradicted the earlier headless gate's coverage. The followup strengthened the selector to `html body[data-scroll-locked]` and added a regression that inserts the authentic late 17px important rule after Select and Appearance open, retaining strict geometry/focus assertions and checking zero computed margin/compensation.

Only that CSS selector/comment and its navigation regression changed. Frontend lint, nonincremental TypeScript and production build passed again. `data-scroll-lock-compatibility-20261009/browser-gate.log` records all 26 tests passing across `navigation-motion`, `appearance-state` and `profile-layout` on final image `d1309dd7…`, with verified cleanup of owned `illya-web-smoke-c96bf4e12e4cdd8e` resources. The previous 87-test result remains valid evidence for its original image; this record does not claim an 87-test rerun on the final compatibility image. Six navigation captures were refreshed with new image/source provenance; their originals were archived first. Independent scoring found the known defect resolved, no visible introduced regression and `disposition: ship` for the scored fix. The detector was not rerun for this functional correction.

The final guarded deployment is recorded in `data-scroll-lock-compatibility-20261009/web-deploy-20261009T055148Z-2264bdd442c04399abf5f9da4653e5fe/deployment-manifest.json`. It recreated only Serendipity in client-test then main, using the exact tested image, without builds/pulls/dependency recreation. All eight protected non-Web containers, their identities/images/state/health/timestamps/mounts, storage, schema 22 and original data IDs/count floors were verified preserved. Client data remains two users, 48 maps, two scores and one Replay; main remains two users and empty map/score/replay collections. The initial copied guard's preflight refused to proceed when its immutable migration manifest sibling was absent; copying the original hash-verified file resolved this without any runtime mutation. Subsequent preflight and deployment passed.

Root inspected the actual retained `https://illya.test` page through CUA at 1280×720. Body/header/main width remained 1265px and x=0 before/after Source Select; computed margin and compensation were 0px. Appearance also retained 1265px width, and Escape restored the controls. The header exposes one Appearance entry; dark/light/system remain inside it. Homepage showed the original fully loaded 700×1251 character, pure black canvas, real counts (0 online, two registered, one ranked score, 48 maps), actual score links and Replay. Navigation feedback was visible while entering home and settled to false after commit. No preferences, credentials, uploads, server records, native clients, hosts, trust, firewall or nginx settings were changed. CUA emitted actual screenshots inline; its read-only measurements are recorded in `data-scroll-lock-compatibility-20261009/actual-browser-observations.json` and are kept separate from synthetic filesystem captures. The measurements establish horizontal stability, not a vertical-scroll invariance claim.
