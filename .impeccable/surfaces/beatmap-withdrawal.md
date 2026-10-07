# Beatmap-set withdrawal

Visitor mode: **Operate**. This is a code-led extension of the existing beatmap review queue and detail page. Preserve PRODUCT.md, DESIGN.md, the sidecar and the current appearance controls. No new visual world, concept seed, artwork or mockup.

THESIS: Staff can stop distributing an entire beatmap set and later restore it, while retaining its scores, replays, identities and files.

OWN-WORLD: Keep the plain black default, neutral adjustable thin glass, rounded bilingual type, paired light theme, personalized accent and shared controls. Copy names the action and its consequences directly in zh-TW, en and ja.

STORY: Filter the queue by availability independently of ranking status, inspect retained metadata through an authorized management link, open a whole-set confirmation, record a reason and submit against the server revision. Resolve conflicts by refreshing the availability while retaining the entered reason. An acknowledged write cannot be repeated merely because its refresh failed.

FIRST VIEWPORT: The queue retains its title, search and source/mode/status filters; availability joins those filters and the divider-led difficulty table. A text badge distinguishes a withdrawn set from its unchanged difficulty ranking status. The focused confirmation identifies the set, describes all-difficulty scope and preservation, and provides a labeled reason field with explicit confirm/cancel actions.

FORM: Reuse the existing admin table, glass material, input, textarea and button primitives. A Radix dialog protects focus for an action that changes availability of every difficulty, including official members not yet imported. Retained management details are account-scoped and masked during session loading, logout and access failure. Withdrawn public details explain the state and expose no download actions. Dense table overflow stays local and keyboard-accessible; the dialog scrolls within the viewport.

FINISH: Root performs one batched desktop/mobile inspection and at most one confirmation round, one detector run, and independent finishing review/documentation. No global token or recorded drift repair is part of this extension.

## Evidence boundaries

Synthetic browser fixtures verify request composition, whole-set dialog language, conflict/retry behavior, postwrite refresh locking, URL/history/pagination, permissions and private-cache clearing. Backend PostgreSQL and protocol gates must establish actual withdrawal across publication paths and retained history; browser fixtures do not prove those policies or native gameplay.
