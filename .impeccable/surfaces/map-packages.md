# Official map-package import

Visitor mode: **Operate**. Extend Serendipity's existing map import, detail downloads and package membership. Retain the approved pure-black default, thin neutral glass, configurable appearance, Nunito/Noto Sans TC, shared controls and ordinary factual copy. No replacement visual world, artwork or slogan belongs to this scope. The existing local package direction and its evidence remain in `local-map-packages.md`.

## Direction contract

THESIS: An account with `beatmaps.import` can choose one official difficulty or provide a complete official `.osz` beatmapset, understand what the server verifies and inspect the returned current difficulty statuses.

OWN-WORLD: Use the existing glass upload form, native select, fields/drop zone, button scale, status/alert hints and divider-led package rows. Reuse the live surface, foreground, border, accent, blur and radius roles; add no global primitive or material.

STORY: Single-difficulty import remains the default. The import-type control makes complete-package import explicit. Switching flows preserves inactive drafts and file selections while clearing old feedback. Success clears only the submitted flow's inputs; failure retains them with actionable localized recovery.

FIRST VIEWPORT: Keep the existing narrow import page and heading. Type selection precedes the selected flow's ID/file fields; its submit action names that flow. The complete-package hint states the verification boundary before submission. Results stay below the form actions and use the existing result divider and paged difficulty rows.

FORM: Direct source implementation on the incumbent Operate surface. Busy state disables the mode selector and all inputs, while the in-flight ref prevents duplicate submissions. Error/status feedback is focusable and announced. Shared Pagination keeps `type="button"` so result navigation never triggers upload validation or a second submit.

## API and source contract

- Both flows require `beatmaps.import`. An upload-only account gains no official-import access; form protection and existing catalogue CTAs retain distinct upload/import grants.
- Single difficulty retains `POST /beatmaps/import` with JSON `{official_id}` and a `Beatmap` response. Its positive integer ID is bounded to 1999999999, and the result retains the single-difficulty semantics.
- Complete beatmapset uses `POST /beatmaps/import/archive` with multipart `official_set_id` and `file`; no JSON content-type is set. The ID is an integer from 1–1999999999. The selected file must be non-empty, end in `.osz` and be at most 64 MiB. These client checks do not replace backend validation.
- The complete response uses existing `BeatmapUploadResult`: `items`, `package`, `idempotent`. Official packages carry `source: 'official'`, UUID `id`, `native_set_id`, `kind: 'osz'`, `legacy: false`, difficulty count/IDs and download URL. Package source is optional in frontend types to retain compatibility with prior local responses.
- Any authorized importer may retry an identical official archive. Exact retry returns existing difficulties with their current moderation statuses and original files; it does not create another package, overwrite them or force a local Pending status. A changed archive/version or conflicting membership gives an explicit conflict.
- `MapUploadPage` retains both flow inputs in hidden wrappers, disabling inactive controls so FormData and native validation use only the active flow. `UploadResult` accepts official semantics for its result title, new-import/exact-retry success and retry hint, while shared rows display the API's actual statuses.
- Official detail consumes the same package shape, `download_kind: 'osz'` and paged package-membership endpoint as local packages. The current difficulty and package page remain separate from score-board mode/category/page state. Complete `.osz` and single `.osu` actions remain explicitly labelled.
- The complete form exposes a translated factual guide link to `/docs/official-archives`. The existing Read container allowlists official archives, native-map discovery and local-map packages, also resolving their relative Markdown document links without changing the document surface.

## Verification and recovery copy

The complete-package instruction explicitly says the server verifies each difficulty's official ID/checksum against osu!. Audio, backgrounds and other assets remain the original uploaded content; the interface does not claim every asset is individually verified by osu!, fetch a full archive automatically for public guests or request an official password.

English, Traditional Chinese and Japanese share flow selection, set/file bounds, new/exact-retry result semantics and actionable known-error copy. The mappings cover `official_archive_invalid`, `official_archive_too_large`, `official_set_version_conflict`, `official_archive_conflict`, `official_archive_manifest_mismatch`, existing official namespace/version/identity/checksum errors, upstream rate limiting, missing operator configuration/authentication, unavailable upstream verification and missing official map/set IDs. The file hint states that `.osu` difficulty files belong at the archive's top level; nested assets remain preserved. Conflict copy preserves existing content; upstream failures retain input and direct a later retry or operator configuration check.

## Finish boundary

The dedicated Chromium fixture file is `apps/serendipity/tests/official-map-packages.spec.ts`. All API requests and supplied upload bytes are intercepted fixtures. It covers default single-ID JSON regression, flow switching/draft retention, multipart fields, ID/file bounds, busy duplicate prevention, current-status new/exact-retry results and safe result paging, official detail membership/downloads, three-language error retention and separate permission grants. It includes opt-in final capture hooks for `workflow-official-map-package-members` and `workflow-official-map-package-retry`; creating or judging final captures belongs to root's bounded browser/reviewer pass.

Root owns integrated source gates, final image/build/browser evidence, served guide-page checks and formal finish review. This source extension does not carry forward the earlier local-package `ship` disposition as approval for new official archive import. Actual archive safety, official verification, ownership/idempotency and preserved byte content require backend/integration evidence; native stable/lazer import/login/gameplay validation remains pending where the project records it as pending.

## Official archive final checkpoint

2026-10-08: source UI f2bbde5 and final website image 3fb96b85 pass thirteen dedicated official-package Chromium fixture cases and the thirty-nine-case combined catalogue/local/official regression. The failed initial fixture assumptions were corrected without changing production UI; every original behavior assertion remains. Eight fresh desktop/mobile dark/light captures and two SHA/provenance layout records show no document-width overflow. A fresh independent generic finish reviewer returns bounded ship, with no material visual fix, using the degraded reviewer contract without a standalone quality-bar/detector packet. The scoped detector and full TypeScript/ESLint passed earlier; this fixture/capture evidence does not certify real metadata publication or native gameplay.

Backend source/PG, pinned-image HTTP, schema-0018 backup/upgrade and real local retained-data evidence are recorded in docs/verification.md. Native-client score persistence remains explicitly unverified.
