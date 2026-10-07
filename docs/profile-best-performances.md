# Profile best performances

Player profiles offer **Best performances** and **Recent scores** for the selected native ruleset and Normal/RX partition. Best is the default; `score_view=recent` selects chronological history. The URL also retains mode, variant and page for browser history. Changing collection or partition starts at page one.

`GET /api/v1/users/{id}/scores/best?mode=0&variant=vanilla&page=1&page_size=24` returns strictly public qualifying performances. A missing or restricted player is 404 for every viewer. Only approved (`verified`), passed, pp-eligible results on ranked maps qualify. One persisted result is selected per map by highest pp, with accuracy, combo and ascending score ID resolving a tie. The complete collection is ordered by pp descending, then map ID, matching the existing profile pp aggregation.

Positions and weights are calculated before pagination. A play at position `n` contributes `pp × 0.95^(n−1)`. The server supplies `position`, `pp_weight` and `weighted_pp`; the website formats them without recomputing them from the visible page. The response retains the existing page shape, plus `mode`, `score_variant`, `collection=best` and `ordered_by=pp`. Original score, standardized score, mods, date and result identity are unchanged. Private ingestion keys and replay file paths are not returned.

Count, selected rows, weighting and player visibility share one SQL statement. A deterministic order does not freeze offset pagination across later legitimate score submissions or review changes; a subsequent page can reflect the updated collection.

Recent continues to use the existing `/api/v1/scores?user_id={id}&mode=...&variant=...` contract. Public viewers see approved results; the owner or a moderator may see additional pending/rejected rows under existing authorization. This preserves the owner's review history. The website keeps recent-data caches scoped to the current account, disables private reads while session identity loads, and clears inaccessible cached rows after access failures or account changes.

Both collections preserve normal/RX separation; RX is supported for osu!, taiko and catch, with mania remaining Normal. Raw or unavailable standardized values are not substituted for pp. This website/API feature does not establish real native gameplay compatibility; verification and deployed image evidence are recorded separately after the gates complete.
