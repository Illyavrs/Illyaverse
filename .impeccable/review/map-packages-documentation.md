# Local map packages: design persistence review

Date: 2026-10-07; final evidence recorded 2026-10-08. A default sub-agent performs the documenter role because the shipped named role is unavailable. This source-led **Operate** extension follows `../surfaces/local-map-packages.md` and preserves the incumbent Serendipity design world.

**Persistence disposition: retain the incumbent system and record the scoped component extension.** `DESIGN.md` adds a local map-package/upload-result subsection under Components. Its frontmatter and existing system rules stay unchanged. The existing `.impeccable/design.json` shared glass, button, badge and control previews remain accurate, with no new primitive token or variant to regenerate. `PRODUCT.md` is read as product authority and is outside this write boundary. **Independent formal visual verdict: [ship](map-packages-verdict.md) for the bounded package-detail, upload/exact-retry, pagination and import-recovery UI scope.**

## Source comparison

| Workflow | Frozen source evidence | Persistence decision |
|---|---|---|
| Actual file-kind downloads | `MapDownloads` in `features/beatmaps.tsx` branches on `Beatmap.download_kind`; complete `.osz` uses package/fallback download and exposes an outline `/osu` action. Single `.osu` uses the difficulty action and the asset-exclusion hint. | Existing primary/outline Buttons and hint typography, with scoped wrapping and a minimum 36px control height. No new token. |
| Local package summaries | `PackageDifficulties` requests `Page<BeatmapDifficulty>` at 24 rows per page, with separate loading/error/retry/empty state and `keepPreviousData: false`. The panel is separate from the score board and displays current version on every page. | Existing `glass content-panel`, heading, catalogue rows and Pagination. Current-page row gains the scoped 8% live-accent mix plus text Badge and `aria-current`, rather than a new brand color. |
| Difficulty identity/status | `DifficultyRows` reuses `map-row` with `package-difficulty-row`, preserving version, title/artist, creator, mode, status, stars and duration. Current row marking is conditional on ID. | Existing mode-icon/status conventions and shared radius. Scoped wrapping preserves metadata, with 52px/36px icon sizes and fixed numeric space. |
| Typed new/exact-retry results | `BeatmapUploadResult` supplies items, package and idempotent. `UploadResult` chooses new versus exact-retry status text, keeps each returned current moderation status, pages the returned items in groups of 24 and labels download by package kind. | Results remain within `glass upload-form`, using shared status/hint/action styles and a border divider. The 22px result heading and scoped spacing are documented as this surface's composition. |
| Result pagination inside the form | Shared Pagination in `components/common.tsx` sets both navigation buttons to `type="button"`. The package regression observes native invalid/submit events and whether the file field receives focus while result pages change. | The corrected button type prevents pagination from invoking the upload form. It is a control-behavior correction, with no visual token or world change; the final run passed the regression. |
| Failure retention and recovery | `MapUploadPage` clears feedback before submission, uses an in-flight ref and disabled controls, and resets the form only after success. Known 4xx errors select localized copy; official import link is grant-controlled, unauthorized offers sign-in, and forbidden gives permission guidance. | Shared `form-error`, `text-link`, hints and Protected patterns. No destructive or success treatment is redefined; selected input is retained on failure. |
| Three-language copy | The zh-TW/en/ja `maps` dictionaries contain the same package/download/result/status/error/recovery keys. | Ordinary factual labels and file extensions; no slogan, artwork or new voice. |

Read-only source sample: `apps/serendipity/src/features/beatmaps.tsx`, `src/lib/types.ts`, relevant `src/app/globals.css` package/download/result rules, `src/components/common.tsx` Pagination/Protected patterns, and `messages/zh-TW.json`, `en.json`, `ja.json`. `PRODUCT.md`, `DESIGN.md`, the document reference and degraded documenter instructions were read. This pass does not run context, detector, browser QA, build or tests.

## Final source, image and functional evidence

Source and product UI are frozen for this documentation pass. Root supplied the following final evidence; existing logs and layout records were read without running new gates. No verification file or source/runtime/test file is owned by this pass.

| Gate | Actual final evidence | Scope |
|---|---|---|
| Rust source | 179 workspace unit tests passed. Sanctuary has 34 tests ignored by default; the separate opt-in run passed all 34 (32 PostgreSQL cases and two native-artifact vectors). Formatting and all-target Clippy passed. | Source and isolated integration/artifact checks, not real native gameplay. [Source log](../../data-map-package-web/source-gate.log). |
| Frontend source/build | Typecheck, full ESLint and final Next Docker build passed. | The final built Serendipity image. [Lint log](../../data-map-package-web/frontend-lint.log), [final build log](../../data-map-package-web/serendipity-build-final.log). |
| Isolated final-image HTTP | 343 HTTP requests and 41 check categories passed in `illya-smoke-cb53f2775db970e3`. | Includes immutable multi-difficulty package/retry/grouping/assets and paged membership/explicit original-format download contracts. [Protocol log](../../data-map-package-web/protocol-smoke.log). |
| Isolated final-image browser | 26 tests passed: 17 map-package and nine catalogue cases, in `illya-web-smoke-b719ff22873ec99b`. | Includes typed new/exact retry, current statuses, package paging/reset/loading/retry, file-kind labels, three-language retained-input errors, permission CTAs, and corrected Pagination invalid/submit/focus behavior. [Final browser log](../../data-map-package-web/web-smoke-final.log). |

The final browser and HTTP logs pin these application images:

| Service | Final image identity |
|---|---|
| Sanctuary | `sha256:13f03e973cdcf091f5514b2f4f5afd9ab653b66b8e109fc17a11fd78114d9ce0` |
| Resonance | `sha256:767f2e37f8814b87c16a59af78d409c41a2ce4e232b2109d62e6013b810ce1dd` |
| Serendipity | `sha256:7d272dd617a5402fe5d2d350e49a213fa7dfd0192881afba53708469b7897f53` |

## Supplied fresh captures

The eight fresh captures were supplied from the final browser run at 2026-10-07 15:57 UTC. The earlier 26-test run omitted the capture flag; its passing count is not evidence that it produced these images. This documenter pass records their provenance and layout metadata; the independent reviewer owns the formal visual judgment.

| State | Desktop dark | Desktop light | Mobile dark | Mobile light |
|---|---|---|---|---|
| Package membership/current difficulty | [Capture](workflow-map-package-members-desktop-dark-en.png) | [Capture](workflow-map-package-members-desktop-light-en.png) | [Capture](workflow-map-package-members-mobile-dark-en.png) | [Capture](workflow-map-package-members-mobile-light-en.png) |
| Exact retry/current-status result | [Capture](workflow-map-package-retry-desktop-dark-en.png) | [Capture](workflow-map-package-retry-desktop-light-en.png) | [Capture](workflow-map-package-retry-mobile-dark-en.png) | [Capture](workflow-map-package-retry-mobile-light-en.png) |

[Membership layout JSON](workflow-map-package-members-layout.json) and [retry layout JSON](workflow-map-package-retry-layout.json) each record all four captures. Desktop viewport/document width is 1440px; mobile viewport/document width is 390px; all scroll positions are zero and recorded theme matches the filename. Both records identify isolated intercepted API fixtures with no real accounts, package bytes or writes.

**Independent formal visual verdict: [disposition: ship](map-packages-verdict.md).** The reviewer opened all eight fresh captures, confirmed persistence and fidelity to the incumbent system, accepted the responsive row adaptation and found no open material fix or regression. The [structured findings](map-packages-findings.json) record the resolved P2 upload-result pagination issue and scope limits. English captures supply the fresh visual evidence; Traditional Chinese and Japanese are supported here by source and passed functional fixtures. This disposition covers the stated Operate extension, not complete-product or native-client certification.

## Five-line system summary

1. Palette: incumbent pure-black ground, neutral thin glass, live accent, paired foregrounds and existing status colors.
2. Type: Nunito Variable/Noto Sans TC and the existing heading/body/metadata ramp; scoped result heading is 22px.
3. Layout: shared panels/forms and divider-led rows; package and result lists page independently in groups of 24.
4. Material: unchanged shared radius and independent panel blur/opacity; no new primitive or shadow vocabulary.
5. Controls: actual file-kind labels, text plus current-page state, localized status/alert/recovery patterns preserve direct Operate copy.

## Drift and limits

**Not canonized or repaired:** existing native-control font inheritance, unlayered border-color cascade and unguaranteed sheet animation remain the incumbent observations already recorded in `DESIGN.md`. They are outside this scoped extension; no new global rule legitimizes them. Existing colored mode icon tiles remain a catalogue convention rather than new neutral-glass tokens.

Source-backed capability, isolated integration/HTTP passes and intercepted browser fixtures are distinct evidence. The final gates support their specified contracts and corrected pagination behavior; they do not certify deployment, real-account workflows, stock stable/lazer import/login/gameplay or whole-product completion. Native-client pending states remain pending. No shipping raster or generated artwork is introduced.
