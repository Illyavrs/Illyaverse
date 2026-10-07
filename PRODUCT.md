# Illyaverse

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

User-selected Next.js, Tailwind CSS, shadcn/ui, Rust, Docker deployment and rosu-pp. User-selected next-intl internationalization with a cookie preference and persisted account locale.

## Users

osu! players joining a publicly registrable community, and operators maintaining accounts, permissions, beatmaps, scores, replays and chat. Support both stable and lazer; protocol completion requires real client verification.

## Product Purpose

A self-hostable osu! community whose players can make the site and their profiles feel personal, connect their client with clear instructions, discover maps, compare scores and talk to each other.

## Operating Context

The monorepo lives in osu-ps/Illyaverse. Operators deploy containers, configure their domain, certificates and upstream credentials; players register, follow client-specific connection instructions and use community features.

## Capabilities and Constraints

Public registration. Landing and connection guides. Light/dark themes; uploaded backgrounds, blur, dim, accent and radius controls. Profile banners and customization. Server-enforced permissions with restricted administrative areas. Chat, official beatmap fetching with rate limits, local exclusive beatmap uploads, scores, replays and rosu-pp calculations. All source must be original: upstream projects inform behavior and structure only.

## Brand Commitments

Illyaverse, inspired by 魔法少女伊莉雅. Related project names should form a coherent family like Solar System. osu! official site provides layout inspiration. Anime-compatible frosted glass, black and dark glass by default, rounded typography. User-customized values override defaults.

User correction: no AI-generated art. The default background is plain black; only player-uploaded backgrounds are used. Commit messages follow Conventional Commits with type(scope): subject.

## Evidence on Hand

Reference URLs: SunriseCommunity/Solar-System, osuAkatsuki/bancho.py, GooGuTeam/g0v0-server and ppy/osu. No supplied art or live server activity; do not fabricate player counts or finished client compatibility.

## Product Principles

- Personalization works without compromising readable content.
- Data and capability claims reflect the running server.
- Deployment and connection instructions are reversible and explicit.
- Permissions are enforced at every protected API.

## Open Decisions

Initial locales zh-TW, en and ja are an implementation assumption; more can be added. Production domain and upstream osu! OAuth credentials must come from the operator. Deployment scale is unspecified.
