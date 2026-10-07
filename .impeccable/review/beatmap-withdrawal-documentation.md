# Beatmap-set withdrawal documentation

This report records the built withdrawal/restoration extension and its bounded finishing evidence. It is a surface record, not a new design-system definition.

Substitution: the dedicated reviewer and documenter roles were unavailable. This documentation pass is an independent generic substitute following the supplied degraded Impeccable documenter instructions; the review packet also discloses its independent generic reviewer substitution.

## Scope and preserved authority

The [surface brief](../surfaces/beatmap-withdrawal.md) defines an **Operate** extension of the existing administrative beatmap queue and detail page. [PRODUCT.md](../../PRODUCT.md), [DESIGN.md](../../DESIGN.md) and the [design sidecar](../design.json) remain the incumbent authority. This pass created only this report. It did not edit those files, implementation, translations or captures, and did not run a browser, tests, detector, context, seed or doctor workflow.

No art assets, approved compositions, new visual world, global design-system changes or broad redesign belong to this extension. The default black ground, paired light theme, adjustable neutral glass, user accent and radius controls remain in place. The whole-set workflow is documented here rather than promoted into global visual rules.

## Five-line system summary

1. Palette: semantic black/neutral glass in dark mode, neutral gray/white surfaces in light mode, personalized accent and text-bearing availability/status badges.
2. Type ramp: incumbent Nunito Variable with Noto Sans TC/sans-serif fallbacks; shared body/label/metadata hierarchy, with the scoped dialog and availability summary headings at 22px in source.
3. Named rules: the Personal Accent Rule and Paired Theme Rule keep existing semantic roles and light/dark pairings authoritative.
4. Named rules: the Bilingual Breathing Rule and Local Overflow Rule preserve translated wrapping, intact labels and bounded keyboard-accessible dense tables.
5. Named rules: the Glass Before Shadow Rule, Independent Material Controls Rule and Shared Radius Rule preserve live surface, blur, opacity and radius behavior.

## Implemented surface behavior

Source observations are from [admin.tsx](../../apps/serendipity/src/features/admin.tsx), [beatmaps.tsx](../../apps/serendipity/src/features/beatmaps.tsx), the scoped availability styles in [globals.css](../../apps/serendipity/src/app/globals.css), and the `mapAvailability` messages in [en](../../apps/serendipity/messages/en.json), [ja](../../apps/serendipity/messages/ja.json) and [zh-TW](../../apps/serendipity/messages/zh-TW.json).

- The administrative queue adds an independent `active`/`withdrawn`/`all` availability filter, table column and text badge. Existing search, mode, source, ranking status and pagination remain part of its URL-backed state. Ranking-status changes remain difficulty-level actions; withdrawal and restoration are set-level actions.
- Queue links open authorized management details and carry a bounded return route. Management requests are keyed by account ID, require `admin.view`, mask retained data during session loading or access loss, and discard the relevant cached response on access errors. `beatmaps.moderate` separately controls mutation affordances.
- The shared Radix dialog identifies the map and canonical set ID, shows the current revision, labels a required staff reason, and offers explicit confirmation/cancellation. The request sends `withdrawn`, `expected_revision` and the trimmed reason to the canonical set availability endpoint. Client validation checks a nonempty reason, up to 500 Unicode characters and 2,000 UTF-8 bytes; server validation remains authoritative.
- Conflict recovery requires an availability refresh while retaining the entered reason. An acknowledged write stays marked saved if the following read fails, so the dialog exposes refresh recovery and prevents resubmission of that acknowledged mutation.
- The scoped dialog is capped at 560px width or viewport width minus 32px, with viewport-bounded height and local vertical scrolling. Actions wrap. The queue's dense table stays within its own labeled, focusable scroll region. These are source observations, not a fresh interaction or computed-style audit.
- Retained management details display ranking status alongside availability, canonical set ID, revision, last reason and timestamp. A withdrawn map has no download action, package-difficulty list or active score board in this view. The read-only hint distinguishes review access from moderation access.
- A public `410`/`beatmap_withdrawn` response produces the direct withdrawn notice. That branch renders no retained metadata or staff reason and exposes no download action.

The [availability contract](../../docs/beatmap-availability.md) describes canonical-set withdrawal across uncached official members, publication/admission paths and retained history. It also distinguishes restoration from ordinary import/cache hits, and qualified pp eligibility from preserved saved scores. Those are documented source contracts; these frontend observations do not establish their execution by a deployed server or native client. An uncached official set can be changed through the administrative API by canonical set ID, but has no cached difficulty row to locate in the website queue.

## Recorded copy fix and review outcome

The [initial full verdict](beatmap-withdrawal-verdict.md) returned `disposition: fix` for one material UI omission: withdrawal and restoration confirmations needed to explicitly say that their all-difficulty scope includes official difficulties not yet cached/imported. “All difficulties” alone did not place that consequence before the operator.

The resulting fix changed exactly the two relevant hints in each of the three locale files: six translations in total. Every withdrawal hint now includes not-yet-imported official difficulties while explaining loss of discovery/download/new-score availability, retention and active pp effects. Every restoration hint includes the same official scope while explaining restored downloads/new scores, qualified pp participation and unchanged ranking statuses.

The [fix verdict](beatmap-withdrawal-fix-verdict.md) returned `disposition: ship`. Its scope is the **single scored UI fix**. It reports the desktop zh-TW confirmation resolved and no fix-induced regression observed in the three exact-path recaptures. It does not claim screenshot verification of English/Japanese confirmations or any restoration confirmation, and it does not establish native/server behavior or actual stable submission. The initial full review and the scored fix review are separate stages; the fix verdict is not a new full-system certification.

## Screenshot observations

This pass opened all three exact required PNGs. [Capture metadata](beatmap-withdrawal-capture-metadata.json) records document-top visibility, fonts ready, verified scroll reset and full-page captures. The visual observations below are limited to the photographed states.

| Exact capture | Recorded viewport / image size | Observed state |
| --- | --- | --- |
| [Desktop dark zh-TW confirmation](beatmap-withdrawal-desktop-dark-zh-confirm.png) | 1280×720 / 1280×4045 | The open withdrawal confirmation names not-yet-imported official maps, identifies fixture set #990700 and revision 0, shows the focused reason field and confirm/cancel actions, and fits within the first viewport. The underlying queue retains its search/filter/table structure and independent availability column. |
| [Mobile light Japanese retained detail](beatmap-withdrawal-mobile-light-ja-retained.png) | 390×844 / 390×1105 | The retained fixture displays Ranked and withdrawn together, set #990700 and revision 1, staff reason and timestamp, a management link and the permission explanation. No download or availability-mutation control is present in this captured read-only state. The beatmap-statistics panel remains visible. |
| [Mobile dark English public notice](beatmap-withdrawal-mobile-dark-en-public.png) | 390×844 / 390×884 | The public notice says downloads and new scores are unavailable and existing scores/replays are retained. It shows neither a download control nor the private retained reason. The open black area is the incumbent page ground. |

Fixture names, IDs, reason and timestamp are synthetic evidence. They do not represent retained community accounts, server activity or native gameplay. Motion and keyboard interaction cannot be established from still captures.

## Synthetic browser gate

The metadata records **10 browser cases passed, 0 failed**, using intercepted synthetic API requests. It records web image `sha256:3045e410aa35efae360539d0e22a8b09dffc43e2cac3e426ae08c3d117faf5a9`, root inspection round 2, four capture-gate attempts and one reviewer fix round. Its capture correction reset document scroll to zero; the final exact PNGs were then inspected in a batched desktop/mobile confirmation.

The surface brief assigns the fixture gate to frontend request composition, dialog language, conflict/retry and acknowledged-write refresh behavior, URL/history/pagination, permissions and private-cache clearing. This report records that supplied 10/10 result; it did not rerun or independently reconstruct the tests. Intercepted requests prove fixture behavior only. They do not prove durable database state, authorization enforcement, score publication policy, native protocol behavior, deployed image health or real gameplay.

## Drift and outstanding verification

Not canonized or repaired: the initial reviewer records **0 primary detector findings and 86 incumbent advisories** (60 typography, 20 radius, 6 color), with none in the sampled new availability CSS block. This report attributes those counts to the existing review packet; it did not run the detector. DESIGN.md also already records native-versus-anchor typography and neutral-seam cascade effects as incumbent evidence rather than future rules. These preexisting matters remain outside the ordinary extension and were not repaired or used to legitimize a defect.

No actionable documentation blocker was found in the supplied packet. Native stable/lazer gameplay and real submissions remain outside the UI review evidence. Server/database/protocol gates, final-image/runtime verification and deployment state require their separate [verification record](beatmap-withdrawal-verification.md); this documentation pass makes no completion claim for them. The contract document explicitly keeps implemented source, integration testing, client testing and deployment verification distinct.
