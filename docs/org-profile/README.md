<p align="center">
  <img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/banner.webp" alt="Illyaverse: an osu! community server" width="100%">
</p>

<p align="center">
  <a href="https://github.com/Illyavrs/Serendipity"><img src="https://img.shields.io/badge/Web-Next.js-black?style=flat-square&logo=nextdotjs" alt="Next.js website"></a>
  <a href="https://github.com/Illyavrs/Sanctuary"><img src="https://img.shields.io/badge/Services-Rust-ce824d?style=flat-square&logo=rust" alt="Rust services"></a>
  <a href="https://github.com/Illyavrs/Illyaverse/pkgs/container/illyaverse-sanctuary"><img src="https://img.shields.io/badge/Images-GHCR-2496ed?style=flat-square&logo=docker" alt="Container images on GHCR"></a>
  <a href="https://github.com/Illyavrs/Illyaverse/blob/main/LICENSE"><img src="https://img.shields.io/badge/License-MIT-lightgrey?style=flat-square" alt="MIT License"></a>
</p>

<p align="center">
  <a href="https://shdcloud.top">Live server</a> ·
  <a href="https://docs.shdcloud.top/">Documentation</a> ·
  <a href="https://github.com/Illyavrs/Illyaverse#run-a-local-server">Run your own</a> ·
  <a href="https://github.com/Illyavrs/Kaleidoscope/releases/latest">Get the launcher</a>
</p>

---

Illyaverse brings accounts, beatmaps, scores, replays, rankings and chat into one
deployment. Players register on the website and use the same account in stable or
lazer. The name is inspired by _Fate/kaleid liner Prisma Illya_.

<p align="center">
  <img src="https://raw.githubusercontent.com/Illyavrs/Illyaverse/main/docs/readme/showcase.webp" alt="Illyaverse home page, player profile and beatmap browser" width="100%">
</p>

## Start here

| I want to…                           | Go to                                                                                         |
| ------------------------------------ | --------------------------------------------------------------------------------------------- |
| Play on the server                   | [shdcloud.top](https://shdcloud.top), then connect a client with the launcher below.          |
| Connect stable or lazer              | [Kaleidoscope](https://github.com/Illyavrs/Kaleidoscope), a reversible connection assistant.  |
| Run or deploy my own server          | [Illyaverse](https://github.com/Illyavrs/Illyaverse), with Docker Compose and prebuilt images. |
| Read how it works                    | [Docs](https://docs.shdcloud.top/) and the [integration status](https://docs.shdcloud.top/en/docs/status). |
| Report a problem or suggest a change | Open an issue in the repository the problem belongs to (see below).                           |

## Repositories

<a href="https://github.com/Illyavrs/Illyaverse"><img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/repo-illyaverse.webp" alt="Illyaverse: the monorepo" width="100%"></a>
<a href="https://github.com/Illyavrs/Serendipity"><img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/repo-serendipity.webp" alt="Serendipity: the player website" width="49.5%"></a><a href="https://github.com/Illyavrs/Sanctuary"><img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/repo-sanctuary.webp" alt="Sanctuary: the community API" width="49.5%"></a>
<a href="https://github.com/Illyavrs/Resonance"><img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/repo-resonance.webp" alt="Resonance: the protocol gateway" width="49.5%"></a><a href="https://github.com/Illyavrs/Kaleidoscope"><img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/repo-kaleidoscope.webp" alt="Kaleidoscope: the launcher" width="49.5%"></a>
<a href="https://github.com/Illyavrs/Transfiguration"><img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/repo-transfiguration.webp" alt="Transfiguration: the lazer bridge" width="49.5%"></a><a href="https://github.com/Illyavrs/Docs"><img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/repo-docs.webp" alt="Docs: documentation" width="49.5%"></a>

## What is included

<p align="center">
  <img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/features.webp" alt="Players, website, profiles, beatmaps, scores, chat, staff tools and deployment" width="100%">
</p>

## How the pieces fit

<p align="center">
  <img src="https://raw.githubusercontent.com/Illyavrs/.github/main/profile/images/architecture.webp" alt="Browsers use Serendipity and game clients use Resonance; both talk to Sanctuary, which stores data in PostgreSQL. Kaleidoscope and Transfiguration set up the client." width="100%">
</p>

Each service is built into a container image by GitHub Actions and published to
GHCR, so a server pulls images rather than building on the host.

## Contributing

Illyaverse is a work in progress, and honest status matters: features are marked
implemented, integration-tested, client-tested or pending, and stock stable and
lazer connectivity is only called complete after real-client verification. Commit
messages follow Conventional Commits with a scope, such as
`feat(serendipity): …` or `fix(sanctuary): …`. Start from the
[Illyaverse monorepo](https://github.com/Illyavrs/Illyaverse), which explains how
to build and verify every project together.

<p align="center">
  <sub>Banner artwork is from <em>Sakura no Uta</em> and belongs to its rights holders; it is used for decoration only and is not covered by the MIT License.<br>Illyaverse is an unofficial community project and is not affiliated with ppy Pty Ltd.</sub>
</p>
