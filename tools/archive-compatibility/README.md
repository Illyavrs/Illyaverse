# Archive filename compatibility probe

This original .NET console probe enumerates filenames from tiny, single-entry,
synthetic in-memory ZIPs using the installed, pinned `ZipArchiveReader`. It does
not embed an upstream reader implementation or redistribute client assemblies.

```powershell
./tools/archive-compatibility/test.ps1
./tools/archive-compatibility/test.ps1 -InspectOnly
```

The default assembly directory is `$env:LOCALAPPDATA/osulazer/current`.
`-NativeRoot` can select another installation matching the same exact pins.
The script creates a fresh ignored UUID directory under
`data-archive-compatibility/`, refuses reuse, clears NuGet package sources and
uses isolated build/CLI/temp paths. References are read in place with
`CopyLocal=false`; copied dependency DLLs are rejected. Process environment
changes are restored. Older reports and caches are retained.

Only the reader's stream constructor, `Filenames` and disposal are invoked.
Client Main, GameHost, APIAccess, imports, GUI, login, network requests and player
storage are not used. Read-only PE/IL inspection established that the pinned
game module initializer only registers Realm type metadata. Its two audited IL
body hashes are checked before loading; dependent assemblies are restricted to
the exact audited Framework, SharpCompress and Realm pins and must have no
module initializer. No Realm database instance opens.

## Observed pinned-reader result

On 2026-10-08, all **17 fixtures passed**, with zero build warnings/errors.
Actual runtime: **.NET 10.0.4**. The client bundles **10.0.12**; that runtime was
not executed by this proof. Game assembly: **2026.1005.1.0**.

| Original filename bytes or field | Observed filename |
| --- | --- |
| ASCII `plain.osu` | `plain.osu` |
| Flagged UTF-8 `星.osu` | `星.osu` |
| Unflagged `83 5C` + `.osu` | `ソ.osu` |
| Unflagged `95 5C` + `.osu` | `表.osu` |
| Unflagged `90 AF` + `.osu` | `星.osu` |
| `folder/` + `83 5C` + `.osu` | `folder/ソ.osu` |
| Actual backslash separator before ASCII/CP932 | Backslash remains in reader output |
| Standalone `80` + `.osu` | U+0080 + `.osu` |
| Standalone `A0`, `FD`, `FE`, `FF` + `.osu` | U+F8F0, U+F8F1, U+F8F2, U+F8F3 + `.osu` |
| Valid Unicode Path override | `星.osu` |
| Unicode Path with wrong raw-name CRC | `星.osu` (reader still uses it) |
| Invalid flagged UTF-8 `90 AF` + `.osu` | Two U+FFFD replacements + `.osu` |
| Invalid UTF-8 Unicode Path | U+FFFD replacement + `.osu` |

The first exploratory 12-fixture run matched 9 guessed expectations. Its report
was retained; expectations were corrected to the actual reader observations,
then five standalone-byte cases were added. The official assemblies were never
changed. The server deliberately rejects malformed UTF-8/Unicode Path metadata
instead of copying the reader's permissive replacement/CRC behavior.

Final report: `data-archive-compatibility/b233656f028547348c5a747a4d6d721c/report.json`.

- Report SHA-256: `C5889AD6F409D3C47FB893D0460D8126E41B0AAB3E62B255ABA48DEAC189CD13`.
- Probe SHA-256: `EAE3A08CF3FEBE25C02C7808B7D3A73A51A4B01253E97CB5008DE79E7EE657FD`.
- `osu.Game.dll`: `AD9527E411111CB4B4045C76DF61C3474FD872A04AF73160C05D24E44A588249`.
- `osu.Framework.dll`: `93B62F61C72539ED6FDB3C8EBA18D276A74DE8E1C094280B2531F5A76CAEA528`.
- `SharpCompress.dll`: `A2D92F04A0AD698CDC37A5BD27D52F2B763505BFEB9DF63802A7CBFBE313B25F`.
- `Realm.dll`: `663844D062FAF837871F07C2630EF044942A317F150709F8E130038908D32241`.

IL inspection evidence is retained at
`data-archive-compatibility/58bcca828f7d4619916e594d22c07a73/proof.txt`.

This is filename enumeration evidence for these exact assemblies and fixtures,
not real client import, gameplay, score persistence or broad ZIP-producer
certification. Server policy is documented in
local map packages.

## Server checkpoint

The final source gate passes 71 Sanctuary units (18 raw archive cases), 35
owned PostgreSQL cases (13 packages, 12 official archives, 10 exact filenames),
rustfmt and all-target Sanctuary Clippy with warnings denied. The first compile
failed because an inherited `HashSet` import was removed; it was restored.
A passing intermediate gate was retained before the optional-ZIP64 correction;
the final corrected source passed all gates again. No workspace-wide rerun or
real native gameplay claim is implied.

Independent review caught valid ordinary locator-like comments, ZIP64
offset-only/local-only descriptors, central signatures and optional ZIP64
footers with fitting narrow EOCD fields. Original positive fixtures exercise
these, including prefixes, signed/unsigned descriptors and malformed controls.
Final review found no remaining material issue in the bounded change.

Release Sanctuary `sha256:044dbe9f6ac8f7eec0146e71e09545e18082af12a5d9d045a155e6000ec8c06d`
with unchanged Resonance `sha256:f50992f1a2170ab38d1f830269c4eaf8876520a779a67146f0f665326a4ddb1c`
passed 458 HTTP requests/55 categories in disposable project
`illya-smoke-e059bbd173c587ac`, plus 12 official-archive refusal requests/five
categories in `illya-smoke-db2371fa768fd42f`. The 16 new CP932 requests prove
upload/retry identities, stable/lazer filename selectors, original downloads
and malformed-archive non-publication. Both owned projects and private temporary
environments were verified removed. Cold configured official publication is
covered by mock-upstream source/PG cases, not the credential-free image suite.

Ignored evidence includes `unit.log`, `clippy.log`, the three module PG logs,
`build-sanctuary.log`, `protocol-final.log` and `official-archives-final.log` under
`data-archive-compatibility/`. Existing caches, earlier fixtures and backups were
retained. Compilation uses the existing target with incremental caching off.

## Local deployment

Both retained local stacks were quiet before the 2026-10-08T00:58:51Z backup
checkpoint. Writers stopped before fresh custom PostgreSQL dumps and shared-data
archives; PGDMP magic, `pg_restore --list` and `tar -tzf` passed. Backups use
`data-archive-compatibility/before-archive-names-20261008T005851Z-{main,client}.*`.
No restore or old-cache/data deletion was performed.

The Sanctuary pin above and documentation-only Serendipity
`sha256:1310bcdf2017883c46e02f0af04c71ea163dcd147cfc6d80c206fece6773fa48`
are deployed. The web image preserves all ten filesystem layers and runtime
configuration of the already verified base
`sha256:cbac39aed4d63df1a9af4059a48b054a7bcde1876182cfa40c0a9a9738878f66`,
adding exactly one documentation layer; Next application source was unchanged
and not rebuilt. The previous app's prerender manifest contains no documentation
routes, and actual HTTP checks confirm the new runtime-loaded text and anchor.

All ten services are healthy. The six gateway/PostgreSQL/Caddy container IDs
and images are unchanged; gateways were stopped and started using their same
containers. Schema remains successful 0001–0020. Main retains 2 users/0 maps;
client-test retains 2 users/44 maps. Both retain 0 scores and replay paths.
Core user/preference, map, filename-alias, score, community and availability
fingerprints/counts are unchanged. No synthetic fixture was written to either
retained database.

Twenty read-only HTTP assertions pass across localhost and CA-verified
`https://illya.test`: catalogue/chat/auth boundaries and six updated guides.
TLS uses the existing public CA and loopback resolution only for each request.
Evidence is in `deployment.log`, `runtime-verification.log`,
`retained-container-identities.json`, `runtime-after.json`, `web-docs-image.json`
and the dated backup snapshots. No game, hosts, trust or security setting was
controlled. The pending real stable scalar-limit 413 remains unresolved until
fresh human-operated submission evidence is available.
