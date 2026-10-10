# Launcher and bridge Release builds

Two manual CI producers create verified Release inputs. Kaleidoscope builds from
the complete Illyaverse parent checkout; Transfiguration builds in its own
repository. Launcher packages and bridge DLLs keep separate release channels and
the existing submodule structure.

## Producers and source identity

| Producer | Workflow | Protected environment |
| --- | --- | --- |
| Complete GUI package | [Illyaverse build-kaleidoscope.yml](../.github/workflows/build-kaleidoscope.yml) | `launcher-build` |
| Bridge production input | [Transfiguration build-production.yml](https://github.com/Illyavrs/Transfiguration/blob/main/.github/workflows/build-production.yml) | `transfiguration-build` |

Configure required reviewers and trusted dispatch refs on both environments
before enabling the runner. Both workflows use `workflow_dispatch`, read-only
repository permissions and runner labels `[self-hosted, Windows, X64, illyaverse-native]`.
They upload build inputs and provenance; publication is a separate operation.

The launcher producer checks out the dispatched parent `github.sha` and recursive
submodules. Its receipt requires a clean parent and all five clean child HEADs,
each matching its parent gitlink, before and after packaging. Dispatch inputs
`version` and `bridge_version` must be canonical stable `major.minor.patch`
versions matching the compiled launcher and bundled bridge configuration.

The bridge producer accepts a full lowercase 40-character `source_commit` that
must equal both the dispatch `github.sha` and the clean checkout HEAD. Source text
copied into its component payload must match the exact Git blob bytes. Both
producers disable automatic CRLF conversion for their checkouts.

## Dedicated Windows runner

Provision a Windows x64 runner with PowerShell, Git, Node.js **24**, npm and the
**.NET 10 SDK** already installed. Launcher packaging also requires the MSVC
compiler, Windows SDK resource compiler (`cl.exe` and `rc.exe` on `PATH`) and a
Rust toolchain targeting `x86_64-pc-windows-msvc`. The detached proof runtime is
self-contained and pinned to **.NET 10.0.12**.

Supply a separate compatible stock lazer installation. Its assembly hashes must
match [compatibility.json](https://github.com/Illyavrs/Transfiguration/blob/main/compatibility.json):
`osu.Game.dll`, `osu.Framework.dll`, `osu!.dll`, `System.Net.Http.dll` and
`System.Net.WebSockets.Client.dll`; the detached proofs also require `Realm.dll`.
Both producers freeze the complete top-level DLL inventory and verify it
again after building and running proofs. Native references, source checkout and
fresh output directories must be separate. Stock assemblies and player data are
excluded from uploaded production payloads.

## Build configuration

Set these GitHub environment variables for `launcher-build`:

| Variable | Required value |
| --- | --- |
| `ILLYAVERSE_NATIVE_ROOT` | Existing compatible lazer assembly directory on the runner |
| `KALEIDOSCOPE_UPDATE_REPOSITORY` | `Illyavrs/Kaleidoscope` |
| `KALEIDOSCOPE_UPDATE_PUBLIC_KEY` | Nonzero Ed25519 public key: 64 lowercase hexadecimal characters |
| `TRANSFIGURATION_UPDATE_REPOSITORY` | `Illyavrs/Transfiguration` |
| `TRANSFIGURATION_UPDATE_PUBLIC_KEY` | The bridge channel's separate public key in the same format |

The workflow supplies `KALEIDOSCOPE_UPDATE_VERSION` from `version` and
`TRANSFIGURATION_BUNDLED_VERSION` from `bridge_version`. It runs locked `npm ci`,
the Release package builder, directory/ZIP verification and compiled launcher
diagnostics. The receipt verifies the launcher channel and bundled version; the
bridge channel's repository/key are recorded build inputs without a compiled
bridge-channel diagnostic.

For the bridge producer, provision `ILLYAVERSE_NATIVE_ROOT` as an **OS environment
variable on the dedicated runner**, using an absolute local path. It is not a
dispatch input. `verify-production.ps1` passes that path as `-NativeRoot` and the
MSBuild `NativeRoot` property. The bridge build does not consume the launcher's
channel variables. Private signing keys never enter either producer.

## Artifacts and proof scope

| Artifact | Contents |
| --- | --- |
| `kaleidoscope-verified-gui-package` | Exactly `package/` and `package.zip`, containing the complete Release GUI payload |
| `kaleidoscope-build-provenance` | Source/native snapshots, detached proof logs and `build-receipt.json`, binding source pins, public configuration, package hashes, runtime bytes and compiled diagnostics |
| `transfiguration-verified-production` | Exact ten-file `production/` payload plus `production-verification.json` |
| `transfiguration-production-provenance` | Frozen source/native inventories, command receipts, detached proof logs and completion provenance |

Launcher artifacts are retained for 14 days; bridge artifacts for 30 days.
Failed bridge runs preserve available provenance separately from release input.

Both producers require four successful detached modes:
`--production-browser-only`, `--production-logout-contracts`,
`--production-welcome-contracts` and `--production-relax-contracts`. The launcher
receipt runs them against the exact production DLL inside its package; the bridge
producer uses its freshly built component DLL. Receipts bind that DLL, the frozen
native inventory and proof-runtime bytes, then verify that all remain unchanged.

The launcher receipt includes all four results. Bridge provenance records them
in `detached-proof-results.json`; its release verification JSON retains the
browser/logout schema. Welcome uses pinned metadata and synthetic callbacks;
Relax checks pinned metadata and inert request builders. These proofs do not run
the game or send requests. Native GUI, actual login/logout, gameplay, replay
playback and launcher cleanup remain manual client checks.

## Independent release channels

The [Kaleidoscope publisher](https://github.com/Illyavrs/Kaleidoscope/blob/main/.github/workflows/publish-update.yml)
consumes a successful parent run with the exact parent HEAD and package artifact.
It requires the parent's Kaleidoscope pin to match the selected `v<version>` child
tag, validates package bytes and compiled trust configuration, then signs through
the protected `kaleidoscope-updates` environment. It creates the complete launcher
release described in [update setup](../tools/kaleidoscope/docs/updates.md).

The [Transfiguration release workflow](https://github.com/Illyavrs/Transfiguration/blob/main/.github/workflows/release.yml)
consumes a successful same-repository production run whose HEAD equals the
reviewed component commit. The operator supplies the DLL, source compatibility
and verification SHA-256 values. Unsigned review is the default; explicit signing
uses the separate `transfiguration-release` environment and key, with optional
draft release creation. A bridge update does not require rebuilding Kaleidoscope.

## Current verification boundary

Workflow definitions and synthetic guard tests are present; actual CI producer
runs and production Release packages remain pending. Signing-key configuration,
GitHub source publication and public releases are also pending. The existing
local GUI package remains unsigned Debug. Its earlier Release build failed when
Cargo could not spawn the `serde_core` build script with Windows error 5; the cause
remains unknown. These new producers do not establish that the local failure is
resolved or that native client testing passed.
