# Local map packages

Visitor mode: **Operate**. Scope: Serendipity beatmap detail, local map upload, and their existing official-import recovery path. Preserve the approved pure-black default, thin neutral glass, Nunito/Noto Sans TC, live accent and radius, direct labels and three supported locales. This is an ordinary extension; no replacement visual world, AI artwork or slogan is introduced.

## Direction contract

THESIS: Players can identify the file they will download, inspect the difficulties belonging to a local package, and understand whether an upload created content or returned an existing exact package.

OWN-WORLD: Retain neutral black/gray glass and the existing shared controls, typography, semantic status colors and compact catalogue rows. Package composition and upload feedback belong to these Operate surfaces, not to a new brand treatment.

STORY: The detail page names `.osz` versus `.osu` from the API's `download_kind`. Package membership is a separate paged list with an explicit current difficulty. Upload feedback distinguishes new creation, exact retry and actionable failure; errors keep the chosen file available for retry.

FIRST VIEWPORT: Keep map identity and download actions in the existing detail cover. The package panel follows the statistics and precedes the independent score board. Keep the upload drop zone and submit/back actions in the existing narrow glass form; render results below them with a divider.

FORM: Source-led extension using shared glass, Buttons, Badges, status text, hints, Loading, LoadError, Empty and Pagination. Preserve existing score mode/category behavior and avoid claims of complete native-client compatibility.

FINISH: Root owns the bounded finish review, fresh captures and functional evidence. The final source/image/browser evidence is recorded below; the independent formal visual disposition is [ship](../review/map-packages-verdict.md) for this bounded UI scope. No new shipping raster belongs to this UI scope.

## Source and interaction contract

- `apps/serendipity/src/features/beatmaps.tsx` owns `MapDownloads`, `PackageDifficulties`, `DifficultyRows`, `UploadResult` and `MapUploadPage`. `apps/serendipity/src/lib/types.ts` separates `BeatmapDifficulty` summaries from `BeatmapPackage` metadata and defines `BeatmapUploadResult` as `items`, `package` and `idempotent`.
- `MapDownloads` checks `download_kind`, not merely package membership. For `osz`, its primary link uses the package download URL when available and its outline link targets the current difficulty's `/osu` endpoint. For `osu`, only the current-difficulty download is offered. The single-file hint states that music, images and other difficulties are excluded.
- A detail with package metadata renders `glass content-panel map-package`. `PackageDifficulties` fetches summary rows from `/beatmaps/{id}/package/difficulties?page={page}&page_size=24`. It keeps previous-page data out of the pending state, displays loading/error/retry/empty states locally, and paginates using the response's total and page size. Its state is independent of score-board pagination, mode and score category.
- `package-current` shows the current version above every package page. `map-list package-difficulty-list` contains `map-row package-difficulty-row` links. The matching row sets `aria-current="page"` and displays an outline current-difficulty Badge. Each summary retains version, title/artist, creator, mode, current status, stars and duration; it does not render nested package detail recursively.
- Local uploads accept `.osu,.osz` and expect the typed upload result. `UploadResult` branches on `idempotent` for new-upload versus exact-retry success and uses each returned difficulty's current status. Its existing rows are paged locally in groups of 24. Package `kind` determines its download label and the single-file hint.
- Shared Pagination navigation explicitly uses `type="button"` inside the upload form. Changing result pages must not trigger native file validation, submit the form again or move focus to the cleared file field. The final browser regression observes invalid/submit events and focus as well as the displayed page.
- The submit ref prevents a second in-flight submission; pending state disables the file/input and submit action. `form.reset()` runs only after success. Validation, conflict, authorization and service failures leave the chosen file/input intact. Changing the chosen file or official ID clears old success/error feedback.
- Known client errors use direct localized messages for official upload, package conflict, official namespace/version conflict, unauthorized and forbidden states. An official-map upload error offers the import link only with `beatmaps.import`; otherwise it gives the permission hint. Unauthorized feedback offers sign-in. `Protected` gates upload/import forms with their distinct permissions, and catalogue CTAs use the corresponding grants. This source record does not independently certify server authorization.
- `maps` keys in `messages/zh-TW.json`, `messages/en.json` and `messages/ja.json` supply package/current-difficulty, download, result, exact-retry, status and recovery copy. Preserve factual file extensions and distinctions through translation.

## Local layout and shared tokens

All containers use the incumbent `--surface`, `--border`, `--foreground`, `--secondary-text`, `--radius` and `--panel-blur` through shared `glass`/content/form styles. The shipped defaults remain 8px panel blur, 55% panel opacity and 14px radius; this feature adds no appearance primitive.

| Source class | Actual scoped treatment |
|---|---|
| `map-downloads` | Flexible basis of 340px with `min-width: 0`; stacked shared actions. Buttons permit wrapping with `height: auto`, minimum 36px height and line-height 1.5. At 600px and below the group takes full width. |
| `map-package`, `package-current` | 28px bottom spacing. The persistent current-version line has no text measure cap and can wrap anywhere. |
| `package-difficulty-row` | Existing map-row dividers and 4% accent hover. The current-page row uses an 8% live-accent mix. Titles/metadata wrap; mode text stays on one line and the numeric column does not shrink. Icon tiles are 52px, then 36px at 600px and below; mobile gap is 12px. |
| `package-mode`, package tags | Mode text is weight 700. Package tags use 12px and their current-difficulty Badge uses 11px without a maximum width. Mobile version text is 16px and supporting metadata is 12px. |
| `map-upload-result` | 28px top margin, 24px top padding and one border-role divider. The result heading is 22px; hints/actions gain 12px top spacing. Download buttons use the same wrapping rule as detail downloads. |
| `map-upload-error` | Shared alert copy can wrap anywhere; adjacent hints/recovery links keep 20px bottom spacing. |

The existing mode-colored icon tiles and text-based ranked/loved/pending states are retained as catalogue conventions. They are not a replacement for the neutral glass palette or a new set of global appearance tokens.

## Evidence and capability boundary

Final evidence supplied by root and read from the existing artifacts, recorded 2026-10-08:

- Source gates: 179 workspace unit tests passed; Sanctuary's 34 default-ignored tests were separately opted in and passed (32 PostgreSQL cases and two native-artifact vectors). Formatting, all-target Clippy, frontend typecheck, full ESLint and the final Next Docker build passed.
- Final isolated HTTP gate: 343 requests across 41 check categories passed in `illya-smoke-cb53f2775db970e3`, against the pinned final Sanctuary/Resonance images. It includes package immutability/retry, native grouping, assets, paged website membership and explicit original file formats. See [protocol log](../../data-map-package-web/protocol-smoke.log).
- Final isolated browser gate: 26 tests passed (17 package and nine catalogue cases) in `illya-web-smoke-b719ff22873ec99b`, against the final Sanctuary/Resonance/Serendipity images. The corrected Pagination button-type behavior and native invalid/submit/focus regression passed in this run. See [final browser log](../../data-map-package-web/web-smoke-final.log).
- Eight fresh English captures from the final run were supplied at 2026-10-07 15:57 UTC: package membership and exact-retry result, each at desktop/mobile and dark/light. The [membership layout record](../review/workflow-map-package-members-layout.json) and [retry layout record](../review/workflow-map-package-retry-layout.json) show document widths equal to their 1440px/390px viewports, scroll position zero and matching themes. The earlier 26-test run omitted the capture flag; it is not the provenance of these eight captures. Full image identities and capture links are in the [documentation review note](../review/map-packages-documentation.md).

The independent reviewer returned [disposition: ship](../review/map-packages-verdict.md), inspected all eight fresh captures and found no open material fix or regression in this bounded package-detail, upload/exact-retry, pagination and import-recovery UI scope. The [structured findings](../review/map-packages-findings.json) record the resolved P2 pagination issue. Fresh visual evidence is English only; Traditional Chinese and Japanese are supported here by source and passed functional fixtures. The browser captures use isolated intercepted API fixtures, with no real accounts, package bytes or writes; they are not live community activity. This documentation pass reads supplied evidence and does not initiate new QA, captures or builds.

Complete `.osz` and single `.osu` labels describe returned file contents. The isolated integration and HTTP results exercise their recorded contracts; they do not certify stock stable/lazer import, login or gameplay. Native-client verification remains pending where the project records it as pending.
