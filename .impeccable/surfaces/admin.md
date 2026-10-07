# Serendipity admin surface brief

Scope: `apps/serendipity/src/features/admin.tsx`, its narrow admin additions in `apps/serendipity/src/app/globals.css`, and the `admin` message namespaces in `apps/serendipity/messages/zh-TW.json`, `en.json` and `ja.json`. Visitor mode: **Operate**.

Audience and job: operators who maintain player accounts, roles, beatmaps, scores and audit records. The surface lets an operator inspect the records available to their grants, search accounts, change delegable memberships, manage custom roles, moderate records and revoke sessions while keeping server restrictions visible and understandable.

## Direction contract

THESIS: Make administrative capability legible at the point of action: the shell establishes access, the section controls reveal the records the operator can use, and each row keeps its permitted write or read-only state beside the record.

OWN-WORLD: Extend the incumbent pure-black ground, plum glass, rounded Nunito Variable with Noto Sans TC fallback, semantic accent, quiet seams and runtime-personalized radius. Preserve the compact operational density and divider-led role rows. No new identity, decorative illustration, raster art or AI artwork.

STORY: Enter through the `admin.view` shell, choose an available administrative area, inspect the records, search users when needed, then perform a permitted moderation, membership, role or session action. Protected actions explain their target and constraints, confirm destructive intent inline, and report whether the write and its follow-up refresh both succeeded.

FIRST VIEWPORT: Keep the existing page heading and subtitle first. Follow with the grant-derived section toolbar, then the relevant search or role controls and the glass content panel. The users view places identity and status beside roles and account actions; the roles view places membership metadata, edit/delete controls and the role form in a quiet vertical list. Desktop preserves the table composition; mobile keeps the reading order, wraps role/action content and confines dense user rows to a labeled local scroll region.

FORM: A code-first Operate extension of the existing Serendipity admin composition. No new identity roll, approved comp, reproduction state or quality-bar card applies. `docs/permissions.md` is the authoritative permission and API contract; this surface documents the frontend expression of that contract.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, this surface brief and the named shipping captures carrying their provenance.

## Interaction and content contract

- The `admin.view` protected shell is required before the page is shown. Sections are derived from grants: users from moderation, role management or session revocation; roles from `roles.manage`; beatmaps, scores and audit from their corresponding grants. Backend APIs independently revalidate every operation.
- User search accepts a literal username fragment or positive integer player ID, caps input at 64 characters and resets pagination. `%` and `_` remain ordinary search characters. The visible hint explains the bound and the clear action restores the first page.
- User role controls preserve server memberships that are stronger, unseen or not delegable across background refresh. An operator may change only roles whose permissions they can delegate; locked roles remain named and visibly read-only. Row-level capability flags further constrain moderation and session actions.
- Role editing combines the shared grant vocabulary with permissions already present in the role draft. Built-in roles retain their names and cannot be deleted; wildcard administrator roles are read-only in this editor; assigned custom roles expose the member count and cannot be deleted until empty.
- Restriction, approval, rejection, role writes and session revocation use the existing compact button vocabulary. Revocation and deletion open named inline confirmations, accept Escape while idle, keep busy controls disabled and return focus to the originating trigger when cancelled.
- Feedback distinguishes a localized write error from a successful write whose revalidation failed. The latter exposes a refresh instruction and blocks subsequent writes until fresh data is available. Common API errors are translated in Traditional Chinese, English and Japanese, including stronger-role, built-in-role, last-administrator, assigned-role, invalid-search, forbidden and unauthorized cases.
- Dense users content uses a keyboard-focusable, labeled region. At mobile width the table remains bounded and horizontally scrollable with a visible localized cue; permission choices reflow from two columns to one. The document itself remains within the viewport.

## Incumbent comparison

This extension introduces no palette, font family, radius, elevation or motion tokens. It uses the runtime semantic accent and foreground roles, plum glass surfaces, existing outline/ghost/primary controls, 4px permission labels, divider borders and the live radius formulas. Ordinary admin sections remain shadow-free; focus uses the existing accent outline and familiar control seam. Typography retains the incumbent Nunito Variable / Noto Sans TC pairing, 15px desktop body, 14px mobile body, 12px metadata and the established heading ramp.

The narrow CSS adds only surface behavior: section toolbar spacing, role-row dividers, bounded role/action widths, search wrapping, inline confirmation treatment, stale-refresh messaging, the mobile scroll cue and one-column mobile permission layout. Existing global design files and primitive tokens remain unchanged. Any broader stylesheet advisories or pre-existing prose drift belong to the incumbent system and are reported rather than repaired here.

## Finish evidence and boundary

The fresh independent finish review returned `ship` with no material fixes. The named shipped documenter/reviewer role was unavailable; a generic scoped substitute followed the degraded documenter and finish-review contracts. The reviewer inspected these four final captures once: `data-rbac-review/desktop-admin-zh-TW.png`, `desktop-roles-zh-TW.png`, `mobile-admin-ja.png` and `mobile-roles-ja.png`. The supplied metrics record 1440px desktop and 390px mobile document widths; the mobile users region is 332px wide with 645px content and a visible scroll cue, while the role permission grid changes from two columns to one.

The evidence packet records 13 full fixtures plus 10 final affected fixtures including the new fourteenth behavior, two bounded capture batches with zero errors or overflow, and a single detector result with zero anti-patterns and 86 advisories from the broader existing stylesheet. These are accepted supplied results and were not rerun during documentation. The verdict and metrics support the scoped frontend behavior and visual finish; fixture identities are synthetic QA evidence. Native osu! client behavior, real-player activity, live deployment and production API activity remain outside this surface review.

See [permissions contract](../../docs/permissions.md) and [RBAC finish verdict](../review/rbac-verdict.md).
