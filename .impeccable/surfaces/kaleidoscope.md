# Kaleidoscope surface brief

Scope: `apps/kaleidoscope`, primarily `src/components/launcher.tsx` and the root route. Related targets are the launcher stylesheet, runtime bridge and Tauri window configuration. Visitor mode: **Operate**.

Audience and job: players with an existing stable or lazer installation who want to enter Illyaverse, see the current process state and recover safely when cleanup is incomplete. This surface owns launch decisions and lifecycle feedback; it extends the existing Illyaverse identity.

## Direction contract

THESIS: A compact connection launcher makes one fixed destination and two installed clients immediately understandable, then keeps preparation, running and cleanup states visible.

OWN-WORLD: Preserve the incumbent black ground, dark translucent glass, rounded Nunito/Noto Sans TC lettering, quiet pink primary actions and restrained outlines. No generated artwork or new visual identity.

STORY: Confirm each installed client path, choose the appropriate client, launch it, then restart or close it. If cleanup fails, stop launch actions and expose recovery.

FIRST VIEWPORT: A small launcher title and community label sit above one glass panel. Its fixed-server row precedes stable and lazer rows, each with a read-only path, location action and primary launch action. A lifecycle/status row and cleanup detail follow below; restart, close or retry appears with the relevant state.

FORM: A precisely specified code-first Operate extension of the incumbent brand, shaped directly as a compact launcher. No concept seed or identity replacement was run. The desktop content is designed for the configured 800 × 720 native window; narrow layouts retain the same reading order.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Constraints

- Present one fixed Illyaverse destination, with no server selector.
- Keep separate stable and lazer installation paths and actions; paths are chosen through the launcher rather than typed into editable fields.
- Keep running, restart, close and cleanup states explicit. Incomplete cleanup blocks launch and leaves retry available.
- Preserve the compact mobile hierarchy, keyboard focus and reduced-motion support. Do not squeeze path/status text into document overflow.
- Label development browser simulation clearly. Production browser views are read-only; native IPC behavior requires the desktop shell.
- Treat the Tauri window dimensions as configuration, not evidence of a completed real Windows GUI or gameplay check.

## Finish evidence and boundary

The fresh independent reviewer returned scoped `ship` with no material fixes. Six final desktop/mobile captures were individually inspected, four browser preview/mock-IPC fixtures passed, lint/type checking/build passed, and one targeted detector run found font-size advisories only. The builder's two inspection rounds are closed. See [launcher verdict](../review/launcher-verdict.md).

The existing global design system is retained. This launcher extension ships no artwork; its screenshots are QA evidence. Browser UI finish is documented, while real Windows GUI and gameplay verification remain pending. Headless pre-main host proofs and native contract/network tests belong to their separate functional boundaries.
