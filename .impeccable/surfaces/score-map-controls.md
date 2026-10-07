# Score replay controls and map review filters

Visitor mode: **Operate**. Code-led local extensions of the existing score detail, score table, public catalogue and administrative map surfaces. PRODUCT.md and DESIGN.md remain authoritative; no new artwork, visual world, system token, concept seed or comp is needed.

THESIS: Players can attach a missing matching replay and understand a score's actual comparison value; staff can find and review a difficulty without losing the active queue.

OWN-WORLD: Retain pure black, adjustable neutral glass, rounded type, shared controls and the paired light theme. Use normal factual labels in zh-TW/en/ja. Accent signals actions and selection.

STORY: An owner sees the missing-replay action beside score details, selects an .osr and gets a recoverable result. Rejected scores say rejected; metadata binding does not claim verified gameplay. Boards show the server's supplied rank and ordering value. Admin search and source/mode/status controls lead to a count, paginated difficulties and explicit save actions; URL state supports returning to a queue. Public discovery can isolate community maps.

FIRST VIEWPORT: Keep the existing score identity and comparison hierarchy. The replay form uses a compact inline section, with filename, limit and action together. Admin filters precede the review table and stay usable on mobile; wide rows use local table scrolling without document overflow. Pending, empty, error, accepted-write/failed-refresh and retry states preserve relevant context.

FORM: Extend incumbent surfaces; leave other admin tabs, account appearance and native client workflows intact. No decorative claim, new global primitive or generated raster.

FINISH: One batched desktop/mobile inspection, at most one root confirmation round, one detector run and fresh review/documentation handoffs. Dedicated roles may use the documented generic substitution. Preserve DESIGN.md and its sidecar and report existing drift without repairing it.

## Contract boundaries

Replay uploads use the existing owner-only route, .osr/16 MiB limit, immutable binding and idempotency. A new attachment resets review to pending; existing identical attachments preserve their current review status. Browser fixtures prove assembled requests and client recovery; separate owned PostgreSQL tests prove policy and persistence. No retained community accounts or games are changed for testing.

Map filtering uses existing q/mode/source/status parameters and status mutation. Version search is added consistently to public/admin count and item queries. Ranking status is not visibility or withdrawal; quarantine/delete are not offered. Native gameplay/real score submission remain separately unverified.
