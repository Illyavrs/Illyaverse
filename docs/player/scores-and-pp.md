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

## pp (performance points)

pp is calculated by the server from your actual judgements and mods. Your total pp is the weighted sum of your best score on each beatmap.

- Mods such as AP, AT and DA do not count toward pp.
- Only your best score on each beatmap is kept.

## Relax (RX)

RX is a **separate score category**. It has its own rankings, pp and statistics, apart from normal scores.

- Supported for osu!, taiko and catch. mania has no RX.
- Just play with the Relax mod; nothing else to set up.
- Leaderboards and profiles can switch between Normal and Relax (RX).

## Comparing scores

lazer and stable calculate raw scores differently. A score page shows the raw score and, when possible, the standardised and Classic scores. If a score does not have enough information to convert, it is left out of cross-client score comparison and ranks by pp only.
