<h1 align="center">Illyaverse</h1>

<p align="center">
  <strong>An osu! community server: a customizable player website, Rust services, and a Windows launcher for stable and lazer.</strong>
</p>

<p align="center">
  <a href="apps/serendipity"><img src="https://img.shields.io/badge/Web-Next.js-black?style=flat-square&logo=nextdotjs" alt="Next.js website"></a>
  <a href="services/sanctuary"><img src="https://img.shields.io/badge/Services-Rust-ce824d?style=flat-square&logo=rust" alt="Rust services"></a>
  <a href="compose.yaml"><img src="https://img.shields.io/badge/Deploy-Docker-2496ed?style=flat-square&logo=docker" alt="Docker deployment"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-lightgrey?style=flat-square" alt="MIT License"></a>
</p>

Illyaverse brings accounts, beatmaps, scores, replays, rankings and chat into one
deployment. Its name is inspired by _Fate/kaleid liner Prisma Illya_. Players
register on the website and use the same account in either game client.

Stable and lazer score submissions, replay retrieval and chat have been reported
working. Replay playback, multiplayer, private messages, Relax, reconnection and
launcher cleanup still need client verification. Client-test now runs schema 29,
with its existing data preserved; the main deployment remains at schema 22. See the
current integration record and
[release checklist](apps/docs/content/docs/status.md) for verification scope.

## Run a local server

Use a fully populated Illyaverse checkout and Docker Engine or Docker Desktop
with Docker Compose **2.24.4 or newer**. Run these commands from this directory.

Linux / macOS:

```sh
sh infra/setup.sh
docker compose up -d --build --wait
```

Windows PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File infra/setup.ps1
docker compose up -d --build --wait
```

Open [http://localhost:3000](http://localhost:3000). Setup creates a local `.env`
with independent random secrets and initial administrator credentials; it
preserves an existing file. After the first successful administrator login,
clear all three `BOOTSTRAP_ADMIN_*` values in `.env` and run:

```sh
docker compose up -d --force-recreate sanctuary
```

The default configuration exposes the website on `127.0.0.1:3000` and the
protocol service on `127.0.0.1:8081`. PostgreSQL and the API remain inside the
Docker network. Named volumes persist the database, uploaded media, beatmap
packages and replays.

For a public server, configure your domain, DNS and ACME contact in `.env`, then
use the HTTPS overlay:

```sh
docker compose -f compose.yaml -f compose.production.yaml up -d --build --wait
```

The [deployment guide](apps/docs/content/docs/deployment.md) covers DNS, HTTPS, environment
settings, backups and restoration. The separate
[local client test configuration](apps/docs/content/docs/connecting-players.md) uses `illya.test`
and a local CA; browser preview alone does not configure game connectivity.

## What is included

| Area     | Features                                                                                                                                      |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Players  | Public registration, account settings, session revocation, email verification and password recovery when SMTP is configured.                  |
| Website  | Next.js, Tailwind CSS, shadcn/ui and next-intl; English, Traditional Chinese and Japanese; light/dark themes and configurable glass surfaces. |
| Profiles | Avatars, banners, backgrounds, accent colors, biography, best performances and recorded rank history.                                         |
| Beatmaps | Official osu! API discovery and imports, server uploads, complete `.osz` packages, difficulty previews and status management.                 |
| Scores   | Stored scores and bound replays, rosu-pp calculations, separate Normal and Relax leaderboards and statistics.                                 |
| Chat     | Public channels, private conversations, friends, blocking and timed moderation across website and client protocol services.                   |
| Staff    | Permission-based administration, roles, account restrictions, score moderation and private audit records.                                     |

Official beatmap search and imports need your own `OSU_CLIENT_ID` and
`OSU_CLIENT_SECRET`; requests share rate limiting and handle upstream `429`
responses. Complete archive acquisition optionally uses an operator-provided
`OSU_ARCHIVE_URL_TEMPLATE`, an HTTPS URL with `{set_id}` in its path; it defaults
to empty, so metadata-only sets remain unavailable for download. Additional
redirect hosts use `OSU_ARCHIVE_REDIRECT_HOSTS`. The client-test deployment
includes this workflow.
Local uploads do not require those credentials. SMTP is optional; the website
reports when email features are unavailable.

The default dark background is plain black. Players can adjust background
blur and dimming, panel blur and opacity, glass coverage, accent color and
corner radius. Fonts are self-hosted: Nunito, TaiwanPearl and Zen Maru Gothic.
The project uses no AI-generated artwork.

Relax supports osu!, taiko and catch. Its scores, performance points and ranks
are separate from Normal; mania has no Relax variant. See the
Relax contract for scoring and replay limitations.

Player ranking requests, the Beatmap Nominator role and the revised review queue
are deployed to the retained client-test environment at schema 29. New local
uploads start in Graveyard in that workflow;
nomination and staff decisions target individual difficulties. The
ranking-request contract describes permissions,
conflict handling and integration status. The main deployment remains at schema
22. Client-test also includes private-message read receipts, the combined server
catalogue and archive acquisition. See the
current integration record for
the deployment boundary and remaining client verification.

## Connect a game client

1. Register on the server website. In-game registration points players there.
2. Extract the operator-provided **Kaleidoscope** GUI package in full.
3. Select the installed stable or lazer executable, save the server's HTTPS
   endpoint in Settings, and launch the game.
4. Sign in with the website account.

Kaleidoscope starts stable with `-devserver`. For lazer, it loads the original
**Transfiguration** bridge before startup and removes its own temporary plugin
after the game exits. lazer keeps its existing beatmap library and gameplay
preferences; server account information is isolated by endpoint. A failed
connection does not fall back to the official server.

The bridge supports the exact client files listed in its compatibility manifest.
A lazer update can require an updated launcher package. The current package is
a local unsigned Debug test build with the production web interface embedded; public
release distribution is not configured. The new 17-file package is complete;
its native GUI and gameplay checks remain pending. The
[launcher guide](apps/docs/content/docs/kaleidoscope.md) links the package and explains
restart, cleanup and recovery. See [connection setup](apps/docs/content/docs/connecting-players.md) and
[protocol compatibility](apps/docs/content/docs/status.md) for verification scope.

## Repository layout

Five independently versioned repositories are pinned as Git submodules. The
parent owns shared crates, migrations, workspace manifests, lockfiles, Docker
configuration and integration documentation.

| Project         | Location                                         | Responsibility                                               |
| --------------- | ------------------------------------------------ | ------------------------------------------------------------ |
| Serendipity     | [`apps/serendipity`](apps/serendipity)           | Player website and staff interface; Next.js / TypeScript.    |
| Sanctuary       | [`services/sanctuary`](services/sanctuary)       | Accounts, permissions, community data and storage API; Rust. |
| Resonance       | [`services/resonance`](services/resonance)       | stable and lazer protocol services; Rust.                    |
| Kaleidoscope    | [`tools/kaleidoscope`](tools/kaleidoscope)       | Windows launcher, Rust core, CLI and Tauri / Next.js GUI.    |
| Transfiguration | [`tools/transfiguration`](tools/transfiguration) | Managed bridge for supported stock lazer builds; C#.         |

```text
Illyaverse/
├── apps/                 Serendipity submodule
├── services/             Sanctuary and Resonance submodules
├── tools/                Kaleidoscope, Transfiguration and archive test tools
├── crates/               Shared domain types and stable packet codec
├── migrations/           Shared PostgreSQL schema
├── infra/                Docker, setup, backup and disposable test tooling
└── docs/                 Player, operator and contributor guides
```

The parent origin is [Illyavrs/Illyaverse](https://github.com/Illyavrs/Illyaverse).
All five submodule URLs and child origins use canonical HTTPS GitHub URLs under
`Illyavrs`. Previous child bare origins remain as `local-cache`; the parent's
previous origin remains as `previous-origin`. This configures repository URLs;
source has not been pushed to GitHub, releases have not been published and signing
keys have not been configured. Pinned commits must be available there before a
fresh recursive clone can be claimed to work.
See the [repository guide](apps/docs/content/docs/architecture.md) for submodule workflows
and the boundaries between source files and generated output.

## Develop and verify

Use Node.js **22 or newer** and Rust **1.96**, matching the service Docker builds.
From the complete parent checkout:

```sh
npm ci
npm run dev
```

The root npm commands target Serendipity. API and protocol services need
PostgreSQL, `DATABASE_URL` and service secrets; `compose.dev.yaml` provides
loopback-only database and API ports. See the [architecture guide](apps/docs/content/docs/architecture.md)
for service startup and routing.

```sh
cargo test --workspace --locked
npm run typecheck
npm run lint
npm run build
```

Kaleidoscope's interface is a separate npm workspace:
`npm run dev --workspace @illyaverse/kaleidoscope`. Building its native Windows
package also needs the MSVC toolchain and .NET 10 SDK; follow the launcher guide.
[Verification records](apps/docs/content/docs/status.md)
document the integration gates. Ignored database tests and protocol fixtures do
not replace actual client testing.

Use scoped Conventional Commits, for example
`feat(serendipity): add profile editor`,
`fix(resonance): validate packet lengths` or
`chore(infra): configure HTTPS deployment`. Implementations are original;
reference projects inform architecture and protocol research. Contributor
requirements are in [AGENTS.md](AGENTS.md).

## Guides

Operator documentation is the bilingual site in [apps/docs](apps/docs):

- [Deployment](apps/docs/content/docs/deployment.md) and [configuration](apps/docs/content/docs/configuration.md)
- [Backup and restore](apps/docs/content/docs/backup-and-restore.md)
- [Architecture](apps/docs/content/docs/architecture.md) and [roles and permissions](apps/docs/content/docs/permissions.md)
- [Connecting players](apps/docs/content/docs/connecting-players.md) and the [Kaleidoscope launcher](apps/docs/content/docs/kaleidoscope.md)
- [Verification status](apps/docs/content/docs/status.md)

Player help shown on the website lives in [docs/player](docs/player). The score conversion derivation is in [docs/score-normalization.md](docs/score-normalization.md).

## License and references

Illyaverse is licensed under the [MIT License](LICENSE). Bundled fonts, flag
graphics and launcher dependencies retain their own licenses and notices.
See font provenance, the
[Twemoji attribution](apps/serendipity/public/flags/twemoji/ATTRIBUTION.txt) and
the [bridge notices](tools/transfiguration/THIRD-PARTY-NOTICES.md).

Research references include [Solar-System](https://github.com/SunriseCommunity/Solar-System),
[Sunset](https://github.com/SunriseCommunity/Sunset),
[bancho.py](https://github.com/osuAkatsuki/bancho.py),
[g0v0-server](https://github.com/GooGuTeam/g0v0-server),
[osu!](https://github.com/ppy/osu) and [osu-web](https://github.com/ppy/osu-web).
Their code is not vendored into this project.
