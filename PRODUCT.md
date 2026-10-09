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

Public registration. Landing and connection guides. Light/dark themes; uploaded backgrounds, independent background blur and dim, independent panel blur and opacity, accent and radius controls. Private appearance preferences preview immediately and remain in the guest's browser; signed-in players explicitly save them to their account. Background uploads require an account. Players can edit biography/country and upload, replace or remove their own avatar/banner; image actions preserve unsaved profile text. Server-enforced permissions with restricted administrative areas. Chat, official beatmap fetching with rate limits, local exclusive beatmap uploads, scores, replays and rosu-pp calculations. All source must be original: upstream projects inform behavior and structure only.

## Brand Commitments

Illyaverse, inspired by 魔法少女伊莉雅. Related project names should form a coherent family like Solar System. osu! official site provides layout inspiration. Anime-compatible frosted glass, pure black and neutral gray-black glass by default, neutral gray/white light surfaces, rounded typography. User-customized values override defaults. The panel material uses independent blur and opacity controls rather than a fixed heavy blur.

User correction: no AI-generated art. The default background is plain black; only player-uploaded backgrounds are used. Commit messages follow Conventional Commits with type(scope): subject. Glass coverage has its own 0–100 slider: the minimum retains glass navigation and floating controls, then identity, sections and content join progressively. Blur and opacity independently adjust the selected glass surfaces. The solid counterparts stay readable on uploaded backgrounds.

Use direct, ordinary product labels and explanatory copy. Avoid poetic marketing taglines. Appearance defaults are 8px panel blur, 55% panel opacity and 14px radius; panel blur ranges from 0–48px, opacity from 0–100%, and radius from 0–40px. Background blur and dim remain separate controls.

Map browsing follows the user's osu-web/Sunrise references through original implementation: aligned card metadata and difficulty tracks, a coherent detail overview, and a restrained homepage hover. Full glass coverage must reveal the chosen scene through adjustable material rather than a fixed opaque search scrim. Normal player downloads are complete `.osz` sets containing available music, images and difficulties; raw `.osu` files belong to protocol/editor workflows. Official maps without retained archives disclose the complete-package mirror source; local raw-only maps explain that a package is unavailable.

## Evidence on Hand

Reference URLs: SunriseCommunity/Solar-System, osuAkatsuki/bancho.py, GooGuTeam/g0v0-server and ppy/osu. No supplied art or live server activity; do not fabricate player counts or finished client compatibility.

## Product Principles

- Personalization works without compromising readable content.
- Panel material and background treatment remain independently adjustable.
- Community and appearance copy uses direct labels that explain available actions.
- Data and capability claims reflect the running server.
- Deployment and connection instructions are reversible and explicit.
- Permissions are enforced at every protected API.

## Open Decisions

Initial locales zh-TW, en and ja are an implementation assumption; more can be added. Production domain and upstream osu! OAuth credentials must come from the operator. Deployment scale is unspecified.
