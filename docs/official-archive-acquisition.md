# Verified official archive acquisition

An operator may configure one complete `.osz` provider. The default is disabled:
`OSU_ARCHIVE_URL_TEMPLATE` is empty. Metadata discovery and individual `.osu`
resolution continue to work independently. Client-credentials osu! OAuth tokens
are used for metadata verification, not represented as permission to download
official archives or as a player's identity.

Set `OSU_ARCHIVE_URL_TEMPLATE` to an HTTPS URL with exactly one `{set_id}` in its
path, then recreate Sanctuary. The hostname must be a DNS hostname, without
credentials, a fragment, or a custom port. For example, an operator's authorized
provider might expose `https://provider.example/beatmapsets/{set_id}/download`.
This example is not a configured or verified service. Operators are responsible
for choosing a provider they can use. No real mirror was contacted by fixtures.

The template hostname is the only permitted redirect hostname by default.
`OSU_ARCHIVE_REDIRECT_HOSTS` adds comma-separated exact HTTPS hostnames, with at
most eight hostnames including the template. Wildcards, IP literals, localhost,
`.local`, userinfo, non-HTTPS redirects, and non-default ports are rejected.
Choose trusted hostnames; configuration does not claim to authenticate a DNS
provider or each asset's original provenance. Player input supplies only a
bounded positive set ID. Neither player credentials nor incoming headers are
forwarded to a provider, and clients never receive a redirect to it.

## Download contract

`GET /api/v1/official/beatmapsets/{id}/archive` returns a same-origin attachment.
It accepts anonymous reads; a supplied Illyaverse session or Authorization
header must be valid both before and after the operation. An invalid supplied
session does not silently become an anonymous read.

Native protocol code uses
`POST /api/v1/internal/beatmapsets/archive`, with `x-illya-service-key` and strict
JSON `{ "official_set_id": 42, "user_id": 123 }`. `user_id` may be omitted for
stable's explicit anonymous read-only download. A supplied positive user ID is
checked before and after the operation. The native gateway separately rechecks
its exact bearer session; the service boundary never fabricates an uploader,
administrator, website session, or import permission.

Successful responses contain the provider's exact ZIP bytes, with
`Content-Type: application/x-osu-archive`,
`Content-Disposition: attachment; filename="illyaverse-official-42.osz"`,
`x-illya-archive-sha256`, `Cache-Control: private, no-store`, and `nosniff`.
There is no `.osu` substitution, re-packed ZIP, partial publication, redirect,
asynchronous download job, or synthesized archive asset.

Every acquired archive must contain the complete official original difficulty
manifest. Verification reuses operator import's original ZIP-name checks and
all difficulty ID, set ID, mode, and exact MD5 checks. Every difficulty must
reference nonempty audio present in the archive. Audio is read to its bounded
declared length and CRC; chart-only, missing-audio, truncated, corrupt, nested
official-difficulty, extra-member and incomplete-member ZIPs are rejected.
Audio CRC/readability is not a codec or playback certification. Metadata cannot
authenticate individual audio, image or video assets as official originals.

New acquisition requires known upstream `availability.download_disabled=false`.
Unknown or disabled upstream policy fails closed. Fresh metadata is obtained
before and after network/cache access; stored difficulty versions, withdrawal,
and supplied identity are rechecked before response. Already retained verified
operator archives keep their existing local, offline-readable contract and
withdrawal checks; this feature does not add an upstream dependency to them.

## Bounds and storage

There are two concurrent acquisition/decompression slots, 32 bounded set
flights, 12 provider network downloads per minute, at most three redirects,
a 64 MiB response ceiling, and a 40-second total operation deadline. Official
metadata still shares the existing OAuth/global request-spacing gate. Provider
429 integer Retry-After is clamped to 1–300 seconds and applies globally to new
provider downloads. Temporary provider/validation failures are cached for five
seconds. Blocking decompression retains its concurrency permit if HTTP times out.

Verified response bytes use a transient memory cache: at most two sets, 64 MiB
total, and a 60-second TTL. Cache hits still recheck metadata policy, identity,
version and withdrawal, and re-verify the manifest/audio. No database table,
beatmap catalogue, archive membership, audit event, native update cursor,
filename alias, owner, or persistent media file is created. Persistent import
remains the separate authenticated `beatmaps.import` operation described in
[official archive import](official-beatmap-archives.md).

Search/detail `archive_download_available` means a retained archive or an
operator-enabled acquisition capability. `archive_download_url` is the bounded
same-origin download route. `local.archive_available` and difficulty
`archive_available` continue to describe retained storage only. Capability does
not guarantee a future provider request succeeds or claim assets are retained.
Official search membership, ordering, cursor, and totals are unchanged.

| Status / code | Meaning |
| --- | --- |
| 401 `unauthorized` | Supplied session/user or internal service key is invalid |
| 410 existing withdrawal code | The set is withdrawn locally |
| 409 `official_set_version_conflict` | Stored difficulties conflict with current metadata |
| 429 `official_archive_rate_limited` | Provider quota, concurrency or provider 429 backoff |
| 502 `official_archive_provider_failed` | Provider response/redirect/network failure |
| 502 `official_archive_invalid` | Response is not a verified complete archive with readable audio |
| 503 `official_archive_unavailable` | Acquisition is disabled, upstream policy is unknown/disabled, or retained file is absent |
| 503 `official_archive_timeout` | The total verification deadline expired |

Implementation, isolated fixture results, deployment and human native-client
verification are separate evidence states. The new source contract alone does
not certify a real provider, native import, audio playback or retained rollout.
