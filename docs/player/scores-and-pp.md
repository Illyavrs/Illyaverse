# Scores, pp and Relax

## How scores work

Scores you finish in the game are sent to this server automatically. Verified scores appear on your profile, beatmap leaderboards and the global rankings.

| Status | Meaning |
| --- | --- |
| Verified | Checked and counted for leaderboards and pp |
| Pending | Not verified yet; not counted on leaderboards |
| Rejected | Ruled invalid by an administrator; not shown on leaderboards |

**stable:** a passed score needs its replay to be published. Failed scores are kept as history only and do not enter leaderboards.
**lazer:** a passed score needs a complete, uninterrupted replay capture to be published.

## Resubmitting a missing score

If a disconnected client did not send a score, export that play as an `.osr` replay in osu! and select **Resubmit a score** from your account menu (or the mobile navigation). Upload your own replay, up to 16 MiB. Its player name must match your account, and the exact beatmap version must be available on this server.

For stable exports, confirm whether you completed the play. For lazer exports, the replay grade determines whether you passed. Every resubmission remains **Pending** until a moderator reviews the replay in the admin score tools. Pending and rejected scores do not count toward rankings. Failed plays never enter leaderboards, even after review. The server calculates pp from the replay summary and stored beatmap; the replay alone does not prove legitimate gameplay.

You may have at most 20 pending replay resubmissions. Retrying the same replay returns the existing score and preserves its review status. If a score already exists but lacks its replay, attach the replay on that score's detail page instead.

## pp (performance points)

pp is calculated by the server from your actual judgements and mods. Your total pp is the weighted sum of your best score on each beatmap.

- Mods such as AP, AT and DA do not count toward pp.
- Only your best score on each beatmap is kept.

## Relax (RX)

RX is a **separate score category**. It has its own rankings, pp and statistics, apart from normal scores.

- Supported for osu!, taiko and catch. mania has no RX.
- Just play with the Relax mod; nothing else to set up.
- Leaderboards and profiles can switch between Normal and Relax (RX).
- RX pp is adjusted from the standard calculation (osu! and catch). The method comes from the open-source Sunrise server; see THIRD_PARTY_NOTICES.md.

## Comparing scores

lazer and stable calculate raw scores differently. A score page shows the raw score and, when possible, the standardised and Classic scores. If a score does not have enough information to convert, it is left out of cross-client score comparison and ranks by pp only.
