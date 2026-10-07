# Local beatmap packages

Sanctuary stores each local `.osu` or `.osz` upload as an immutable package.
The website's difficulty IDs and all score, replay, solo-token and round foreign
keys remain internal database IDs. Native clients use separate public difficulty
and set IDs. Neither upload nor migration rewrites retained `.osu` bytes or MD5s.

## Identity and membership

`beatmap_packages` records a UUID, owner, upload kind, SHA-256 of the exact original
payload, byte length, sorted complete difficulty checksum manifest, archive
reference and persistent native set ID. Each difficulty references that package.
An `.osu` package has one member; an `.osz` package has all its accepted members.
Titles, artist names, creator names and embedded official IDs do not define local
membership. Difficulty moderation statuses remain independent.

New local content must have no positive `BeatmapID` or `BeatmapSetID`; official
content uses the official import flow. This avoids accepting a package whose
retained embedded IDs contradict its assigned local public IDs. Stock lazer's
[GetBeatmapRequest](https://raw.githubusercontent.com/ppy/osu/master/osu.Game/Online/API/Requests/GetBeatmapRequest.cs)
sends both a positive online ID and checksum, and its
[legacy decoder](https://raw.githubusercontent.com/ppy/osu/master/osu.Game/Beatmaps/Formats/LegacyBeatmapDecoder.cs)
reads those online IDs from the retained metadata. Lookup and score/token
bindings must continue rejecting inconsistent identity. Byte-identical retries
of existing legacy packages remain possible, but old content with positive
embedded IDs has a native compatibility limitation; migration does not rewrite
it or claim that a lookup refresh automatically repairs every client import.

Migration 0016 groups previous uploads only when every row sharing an archive
has the same owner and the UUID directory / `package.osz` / per-MD5 `.osu` path
boundary produced by the earlier server. Other rows become singleton legacy
packages. This is a database boundary check, not validation of retained archive
contents. A migration cannot inspect deployment files, so legacy SHA-256 and
length remain unknown. Exact retry must verify the actual retained payload and
complete manifest before recording those values. `legacy=false` means the
fingerprint has been established; it does not describe when a package was made.

Package membership, ownership, native IDs and existing content references cannot
be replaced by retry or moderation. An official row may attach its first missing
archive reference; an existing reference cannot be overwritten.

## Upload retries

The upload result retains `items` and adds `package` and `idempotent`. Website
difficulty detail exposes package membership and an explicit package download
URL without exposing filesystem paths or fingerprints.

- A complete byte-identical payload retried by its original owner returns the
  existing difficulty IDs, current statuses and files, with `idempotent=true`.
  It does not create another upload audit event.
- The same checksum appearing twice in one archive is rejected before publication.
- Another owner's content, partial overlaps and repackaged archives with changed
  assets return a conflict. No existing difficulty is silently moved, adopted,
  overwritten or partially supplemented.
- Authority is checked inside the publication transaction, before resource
  locks, and again before commit. Resource locks use deterministic checksum order.
- Failed requests remove only their own unreferenced staged files. If a sent
  commit has an unknown outcome, retained files are kept for reconciliation.

## Native identifiers and upgrade

Public IDs must fit a positive signed 32-bit integer. Official difficulty and set
IDs stay unchanged within `1..1_999_999_999`. Illyaverse allocates local difficulty
and set IDs from independent sequences in `2_000_000_000..2_147_483_647` without
cycling. The high range is a service policy, not a range reserved by osu!.
An upstream identity entering that range is explicitly refused; deployment must
review namespace policy before accepting it. Unknown high IDs never trigger an
official lookup. An exhausted sequence requires operator action.

Migration 0016 assigns new public IDs to existing local difficulties. Previous
internal-PK native URLs are not kept as aliases, because that would allow them
to identify unrelated official content. Native clients must refresh map lookup
by checksum. Already issued local token URLs may also need a fresh lookup/token;
their internal map bindings and historical scores are retained. This is an
explicit upgrade boundary, not a guarantee that old native URLs keep working.

Persisted lazer playlists and version snapshots update only `beatmapID` using
their exact checksum. Mods, rulesets, playlist identity, frozen-round checksums,
participants and internal foreign keys remain unchanged. A saved member ID
without a checksum retains the old official-first interpretation when ambiguous.
Apply the service/schema upgrade with native traffic stopped, then update both
Rust services before accepting clients.

The migration refuses while a local room score receipt is still `received`.
Allow the prior service to finish that queue before upgrading. Stored submissions
and their SHA-256 digests are never rewritten to make an old intent pass a new
identity check. Replay capture state can safely rebind its single public
`beatmapID` from the token's internal FK; original chunks, bundle hashes and
headers are retained. Clients reconnect using the refreshed canonical identity.

An already processed room receipt can repeat its exact stored JSON and SHA-256
after upgrade, even when its optional public map ID is the previous value.
That exception applies only to the accepted receipt's public-ID comparison;
fresh session/owner, internal/frozen map, checksum, ruleset, mods and other
context checks still apply. A changed receipt conflicts. New submissions and
pending recovery retain strict canonical identity checks.

Stable direct search groups complete archives by canonical set ID, distinguishes
difficulty (`b`) from set (`s`) selectors, and downloads `/d/{native_set_id}`.
Lazer lookup, set detail, search, rooms, score responses and tokens share the same
canonical identity. Local archive download returns the exact original complete
`.osz`; a standalone `.osu` or official set without a retained archive cannot be
advertised as a downloadable `.osz`.

## Verification state

The coordinated source check passes 179 Rust unit/CLI checks, 66 isolated
PostgreSQL integration checks and two existing native-artifact vectors. The
populated upgrade, exact retries, ownership/conflict/cleanup, live authority,
300-member native sets, canonical tokens, rooms and legacy processed receipts
are covered. Rustfmt, all-target Clippy with warnings denied, and six migration
source/checksum tests pass. Final Docker images pass 337 isolated HTTP requests
in 40 categories, including canonical pending map/chart identities and unchanged
ranked score/PP for pending submissions. Both local stacks run those image IDs
and migration 0016 with retained-data fingerprints unchanged. Fresh DB/file
backups and the complete evidence are recorded in [verification](verification.md).
Real stable/lazer map search, download, gameplay and submission require separate
user-operated client checks.
