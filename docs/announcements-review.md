# Announcements implementation review

This project-only audit records the completed announcement extension of the existing Illyaverse website. It is excluded from the public documentation reader. The operational guide is [announcements.md](announcements.md); [permissions.md](permissions.md) and [website-features.md](website-features.md) describe the relevant access and capability boundaries.

The image pins, captures, test totals and rollout below belong to the original announcement checkpoint. Current source (2026-10-09) authorizes the API, panel and in-memory draft boundary with `announcements.manage` alone; the shell exposes any permitted staff section without a blanket `admin.view` requirement. This frontend adjustment is still under verification and is not certified by the older captures or deployed to the retained stacks. Their current schema is 0022; the later 0023/0024 chat/score migrations remain source-only. The historical 0021 rollout and its evidence remain preserved below.

The dedicated Impeccable documenter was unavailable; a fresh default agent read the complete role definition and documenter references before performing this pass. This is an ordinary code-led extension. The incumbent [PRODUCT.md](../PRODUCT.md), [DESIGN.md](../DESIGN.md) and [.impeccable/design.json](../.impeccable/design.json) remain unchanged. This audit introduces no primitive tokens, new visual world or shipping raster.

## Finished scope

- The home side column previews up to three published announcements before the connection steps. Public navigation exposes `/news`; its list shows titles, authors, publication dates, excerpts and twelve-record pagination. `/news/{id}` shows the full plain-text body.
- Authorized operators use `/admin?tab=announcements` to create drafts, edit, explicitly publish, update published text, withdraw and republish. The management list has publication-status filters and pagination. New records always begin as drafts; the creation and publication steps are explicit writes.
- Public list and detail APIs return published records only, including for administrators. Draft and withdrawn detail requests return 404. Private management APIs require `announcements.manage`. The originally captured panel additionally required `admin.view`; current source uses `announcements.manage` alone. The permission editor exposes the grant without adding it to ordinary players or moderators.
- Creation and updates revalidate current authority and session inside the audited database transaction. Updates require the expected revision. Conflicts, expired authority, session deadlines or audit failures do not leave partial writes. The public author object contains only public identity fields.
- The editor keeps authored text after failed writes and conflicts, labels unsaved changes, and requires explicit confirmation before replacing or discarding them. A globally mounted memory vault retains pending title/body and the original record revision across same-document route or tab Back/Forward. Recovery first checks private access and the current record. Changed or missing targets retain recoverable text and block stale writes.
- Logout, account change or loss of the required announcement authority clears retained private text even while the editor is absent. The original review exercised both panel grants; current source tracks `announcements.manage` alone. The private announcement list cache is scoped to the visible authorized boundary. Ordinary list pagination and status history keep the open editor intact.
- Loading, empty, retry, validation, conflict, withdrawal and recovered-draft states have direct labels in zh-TW, English and Japanese. Dates use the selected locale; article content retains the author's language.

Persisted content uses migration `0021_announcements.sql` and the `draft`, `published`, `archived` states. Titles accept 1–160 characters; bodies accept 1–20,000 characters within 64 KiB. The API trims surrounding whitespace, normalizes body CRLF to LF, and rejects unsupported control characters. Bodies preserve ordinary line breaks and render HTML-like input as literal text. Editing a published record retains its publication time; republishing a nonpublic record sets a new publication time.

## Surface contract checked

The complete direction remains in [.impeccable/surfaces/announcements.md](../.impeccable/surfaces/announcements.md). The final artifact was compared with every field, including the history requirement added after the first review.

| Contract | Finished implementation and evidence |
| --- | --- |
| TYPE | Self-hosted Nunito Variable and Noto Sans TC remain the shared fonts. Restrained headings, a bounded reading measure, real date fields and text status labels distinguish title, content and metadata. |
| MATERIAL | Public and private panels reuse semantic neutral glass, quiet border seams, independently adjustable panel blur/opacity, live radius, paired light surfaces and shared keyboard focus. |
| GROUND | The default remains pure black, with no added artwork or decoration. The light article uses neutral gray/white surfaces. Public content is operator-authored text. |
| THESIS | Published server information is readable; operators write, review, publish and withdraw it while drafts remain private and failed/conflicting/history-navigation edits remain recoverable. |
| OWN-WORLD | Original persistent records and workflow integrate with the existing shell, home side column, administrative workbench and permission editor. |
| STORY | Home preview leads to the paginated list and full article. Draft creation, editing, explicit publication, withdrawal and conflict recovery provide the promised progression; empty/loading/error states expose the applicable next action. |
| HISTORY | A user-and-permission-scoped in-memory vault survives same-document Back/Forward. Returning revalidates authority and the latest record, shows recovered unsaved text, and preserves the original revision when the server version changed. Replacement/discard requires confirmation. Logout/account/permission changes clear it globally; there is no browser-storage or implicit API write. |
| FIRST VIEWPORT | Home preview precedes connection steps. Public list and detail lead with direct title/author/date information. Desktop administration separates selection and editing; below 860px the editor is first in a single column. Publication remains an explicit action. |
| FORM | Published-only public APIs and 404 for nonpublic details hold independently of staff status. Management authority, current session, audited transactions and expected revisions govern writes. No automatic posting, outgoing notification, executable HTML or fabricated activity was added. |
| MODES AND FINISH | Public pages use Read, management uses Operate, and the preview extends the existing landing context. One initial detector, one initial batch and one consolidated correction/confirmation sequence were retained. The final review and this audit complete the scoped finish record; no new-world seed, composition selection or shipping-image provenance is required. |

## Incumbent system comparison

Palette: shared live accent and readable-accent roles sit on pure-black neutral glass; light mode uses the existing neutral gray/white foreground pairs. The Personal Accent Rule and Paired Theme Rule apply without a new fixed feature accent.

Type: body remains 15px with 1.7 line-height, reducing to 14px on phones. Announcement article text uses 15px/1.95 and a 72ch maximum, reducing to 14px on phones; its title uses the observed 27–40px fluid size. The Bilingual Breathing Rule is preserved by wrapping long titles, bodies and translated controls.

Layout: public records use quiet dividers within the inherited shell; public list and detail containers are bounded at 1040px and 984px. The desktop editor uses a 0.85fr/1.15fr grid and becomes one column below 860px. Controls and metadata wrap; long body strings wrap within the reading region. The Local Overflow Rule remains the governing page constraint.

Material and shape: `.glass` uses `--surface`, a one-pixel `--border`, `--radius` and `blur(var(--panel-blur))`. Defaults remain 8px blur, 55% opacity and 14px radius. Panel blur/opacity and background blur/dim stay independent; announcement selection uses the existing 0.65 radius factor, while status labels remain pills. The Glass Before Shadow, Independent Material Controls and Shared Radius rules are preserved.

Interaction: shared buttons, fields, Lucide SVG icons, visible focus, disabled actions and localized alert/status feedback remain in use. New-announcement selection focuses the title; selection, cancellation, navigation, withdrawal and latest-version replacement expose the required decisions. The workflow adds no decorative motion or container shadows.

The source comparison sampled the announcement stylesheet and components, memory provider, global stylesheet, appearance/session providers, home and shell, administrative boundary, routes, shared types, documentation navigation and all three message catalogues. It preserves the existing system instead of promoting surface-specific sizes or compositions into system-wide tokens.

## Review and evidence

The `data-announcements/` paths in this report are retained, ignored audit artifacts and helpers in the current workspace. They are not supplied in every portable checkout. Application code, migrations and the underlying CI/test entry points remain the reproducible source of behavior.

[announcements-evidence.json](../.impeccable/review/announcements-evidence.json) binds nineteen source files and five full-page PNGs to these exact images:

| Application | Image SHA-256 |
| --- | --- |
| Serendipity | `e05054dcb4ef2b1acf5b2f4d7702b00069fb71739708d3783479b65ebe3f5ed9` |
| Sanctuary | `fb544fb8bc43fbe5afd7f99161cd592d80e2aa8d397bb5ac2a03ca120d5ef1c2` |
| Resonance | `ff7de263934ef575b827d0284d4e438ebee7d132b7b1d35f2c29263800f72893` |

The final browser fixture was the labelled disposable project `illya-web-smoke-bc87b2855ce0dab4`, served at `http://localhost:19000`. Announcement records and identities in captures are synthetic. They do not demonstrate activity in a retained database. The captures are review evidence, not shipping assets.

| Capture in `.impeccable/review/` | State | PNG SHA-256 |
| --- | --- | --- |
| `announcements-news-1440-dark-zh-TW-populated.png` | 1440×1000, public list, zh-TW/dark | `c2b69ea533249983d834a696861891edf68d791d5f44c03b0ac387f95dfda06a` |
| `announcements-home-1440-dark-zh-TW-populated.png` | 1440×1000, home preview, zh-TW/dark | `e5ef62edd09a1caee8154395561578216a05bd777912a830510ee3fc5e7eb484` |
| `announcements-news-390-light-en-longbody.png` | 390×844, long plain-text detail, English/light | `b066db93db82cbe0f3a30b6ba7154f3f90f1ba0f756a5a7424470c5b5e3c83c2` |
| `announcements-admin-1440-dark-zh-TW-edit.png` | 1440×1000, recovered unsaved editor, zh-TW/dark | `65cad5a41226e142393d74a6d5b33d24e440bb2b70a1797625fa67a5baf46ad0` |
| `announcements-admin-390-dark-ja-conflict.png` | 390×844, retained conflict draft, Japanese/dark | `c51974c04859868460f6190c77b121cee2cf3fef9832dfa9cbfaa3f0ffe61fcd` |

The packet records fonts ready, disabled animations, full-page capture, document-top position and zero horizontal page overflow for all five states. The root checked these properties and opened every image; the fresh reviewer reopened all five original paths in the confirmation review. This documentation pass rechecked all nineteen source hashes and five PNG hashes against the packet with zero mismatches, without a new build, capture or detector.

The initial full review, [announcements-verdict.md](../.impeccable/review/announcements-verdict.md), remains preserved with `disposition: fix`. It identified one material defect: local editor state was lost when same-document Back/Forward left the route or announcement tab. Its initial seventeen-source packet was bound to the earlier Serendipity image `4b4b85858063f9de0d8bee3aab227ab8b9ae3c95b58a84bf807a92b7852a3693`.

After the memory-vault correction, [announcements-fix-verdict.md](../.impeccable/review/announcements-fix-verdict.md) records that finding as resolved and has `disposition: ship`, with no remaining correction regression in the supplied evidence. This confirmation verdict covers the scored history-navigation fix; it does not replace the initial full review or certify the whole community/native-client goal.

## Validation recorded

| Gate | Actual result and limitation | Evidence |
| --- | --- | --- |
| Rust formatting and Sanctuary Clippy | Passed; Clippy used `--all-targets --locked -- -D warnings`. | `data-announcements/rustfmt.log`, `clippy.log`, `source-gates.log` |
| Sanctuary unit suite | 74 passed, 84 ignored; ignored integration tests were not counted as passes. | `data-announcements/unit.log` |
| Resonance unit suite | 56 passed, 63 ignored. | `data-announcements/resonance-unit.log` |
| Actual announcement PostgreSQL 17 integration | Four tests passed against an exactly owned disposable tmpfs database. Covered HTTP authority/lifecycle/paging, raw escaped body bounds, queued authority revocation, session deadlines, two concurrent editors and audit rollback. | `data-announcements/announcements_tests.log`, `source-gates.log` |
| Web lint, TypeScript and scoped formatting | Passed. | `data-announcements/lint.log`, `typecheck.log`, `prettier.log` |
| Final web image build | Completed with the Serendipity pin above. | `data-announcements/build-web-final.log` |
| Migration source integrity | Six tests passed; twenty-one migration Git/LF SHA-384 checksums verified under Windows checkout settings. | `data-announcements/migration-source.log` |
| Public documentation navigation | Ten tests passed, including allowlist, fragment/link handling and escaped HTML. This audit was not added to that allowlist. | `data-announcements/docs-navigation-final.log` |
| Existing availability database boundary | Nineteen assertions passed for the separate 0019→0020 availability transition. This is regression evidence, not an announcement migration claim. | `data-announcements/availability-boundary.log` |
| Final announcement browser cases | All eighteen passed on the final image, including actual owned API lifecycle, draft/publish/withdraw, private visibility, dirty guards, conflicts, history recovery and revocation while away. | `data-announcements/browser-final.log` |
| Final mixed browser batch | 56 passed, one failed, one capture-only case skipped. The failure was an existing profile test reading a CSS variable before the profile data rendered. The test was corrected to wait for the username heading and retry the exact variable assertion. | `data-announcements/browser-final.log` |
| Focused profile confirmation | Five passed on the same final image, with no additional build or captures. Disposable fixture: `illya-web-smoke-b29bcc535c84b771`. Combined with the mixed batch, this successfully covers all 57 unique active cases; no single uninterrupted 57/57 batch is claimed. | `data-announcements/browser-profile-confirmation.log` |
| Initial mixed/RBAC history | Initial batch: 51 passed, one outdated RBAC toolbar assertion failed, one capture-only case skipped. The corrected RBAC suite passed fourteen; the final mixed batch also passed all fourteen active RBAC cases. | `data-announcements/browser-round-one.log`, `browser-rbac-confirmation.log`, `browser-final.log` |
| Retained local rollout and read-only runtime checks | Both local stacks upgraded successfully to schema 0021 and the three exact application pins. Ten service containers were healthy; twenty GET assertions passed; four PostgreSQL/Caddy full IDs, retained counts and fingerprints remained intact. | `data-announcements/deploy.log`, `runtime-final.log` |

In the current workspace, the retained helper `data-announcements/source-gates.ps1` owns, labels and removes its exact PostgreSQL fixture. Reproduction in a portable checkout uses the tracked CI/test entry points and underlying commands: `cargo fmt --all --check`, `cargo test -p sanctuary --locked`, `cargo test -p resonance --locked`, and `cargo clippy -p sanctuary --all-targets --locked -- -D warnings`. With an explicitly owned PostgreSQL fixture supplied through `ILLYA_TEST_DATABASE_URL`, the recorded integration invocation is `cargo test -p sanctuary --locked announcements_tests -- --ignored --test-threads=1`.

The web checks use `npm run lint`, `npm run typecheck`, `npm run test:migrations` and the source navigation tests. `npm run test:web:disposable` runs the owned website fixture against locally pinned images; `ILLYA_WEB_SMOKE_TEST_FILES` selects approved test basenames. Reproducing a fixture run does not authorize testing against real account data or replacing retained stacks.

## Drift retained without repair

The single initial detector in `data-announcements/detector.json` reported zero primary and 94 advisory findings: 67 type-size, 21 radius and six color entries. It predates the history logic correction; no second detector was run or implied. Eight entries concern `announcements.css`: 11px secondary metadata/status text, the 27px fluid title endpoint, 23px/21px/19px contextual headings, and the existing live 0.65 radius factor. The radius formula is already documented in DESIGN.md prose; its advisory reflects the detector's comparison with resolved defaults. The supplied reviews found no material visual failure requiring a type or radius repair in this scope. These local values are recorded as evidence and are not added to the normative token ramp to erase advisories.

Source/document drift already exists outside the announcement extension. DESIGN.md describes the prior landing side column at 290px/250px, while the established `home.css` uses 336px/304px. It describes a custom-accent luminance switch at 0.179, while the current appearance provider compares actual dark/white contrast and falls back to black when both fall below 4.5. Sidecar focus previews use `--user-accent`, while the global focus outline uses the readable `--ring` role. The existing global native-control typography and neutral border-color cascade differences remain as already disclosed in DESIGN.md. These documentation gaps were neither repaired nor canonized by this ordinary extension.

No scoped review found a kicker, invented slogan, gradient heading, glyph icon, unrelated display font, decorative side stripe or new artwork in the supplied announcement renders. No craft-floor refusal was turned into a reusable system rule.

## Limits and runtime status

The memory vault is a current-page navigation safety net. It performs no autosave, localStorage/sessionStorage persistence or background publication. A full refresh or close uses the existing unload guard and then loses the in-memory draft. Recovery requires usable private access; it is not an offline or durable draft feature.

No seed announcement or outgoing chat/email notification is created. Real retained deployments start with their own persisted records or an honest empty state. The browser captures and disposable database tests do not establish real server activity.

The retained local stacks `illyaverse` and `illyaverse-client-test` were upgraded by the ignored current-workspace helper `data-announcements/deploy.ps1`; its log completed successfully. A fresh quiet-native preflight preceded the rollout. Both databases and shared-file volumes have backups stamped `20261008T025058Z`: main/client `.pgdump` files were checked for `PGDMP` format and readable `pg_restore` listings, and both `.tar.gz` shared-data archives were readable. The PostgreSQL and Caddy containers retained all four full IDs. Sanctuary, Resonance and Serendipity were updated to the exact pins above; both Rust applications carry migration 0021. All ten containers were healthy, all migrations 1–21 were successful with zero failed migrations, and core/community/availability counts and fingerprints were retained.

The ignored current-workspace verifier `data-announcements/verify-runtime.ps1` then completed successfully with twenty read-only GET assertions, ten per stack. The checks covered the news page, empty public announcement collections, private endpoints returning 401, missing public API records returning 404, a safe missing-ID website shell returning 200, unsafe-ID validation returning 404, the public announcement guide returning 200 and the private website-features guide returning 404. Both databases contained zero announcements and zero published announcements; replay-path counts remained zero. TLS verification used a request-only CA and `--resolve`, with no hosts-file, trust-store, native-game, account or real-publication changes. These logs establish the local rollout and runtime behavior; no public deployment is claimed.

The broader complete-site goal remains active. Actual stable score-submission retry and lazer gameplay have not been proven by this work, and the deferred capabilities in website-features.md remain deferred.
