<!-- BEGIN:nextjs-agent-rules -->

## This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` (resolved from this file's directory; in monorepos the `next` package may not be visible from the repo root) before writing any code. Heed deprecation notices.

This block is written and re-added by `next dev` — verify at `node_modules/next/dist/server/lib/generate-agent-files.js`. Removing it from a diff only re-creates the uncommitted change; committing it with your work keeps the tree clean.

<!-- END:nextjs-agent-rules -->

# Illyaverse child-project policy

# Illyaverse development

All implementation is original. Reference projects may inform protocol behavior and architecture; do not copy their source. Prefer primary upstream documentation and record important protocol assumptions in docs/protocol-compatibility.md.

Keep honest capability states: implemented, integration-tested, client-tested, and pending are different. Never mark stock stable/lazer connectivity complete without real client verification. Do not render invented activity as live server data.

Do not generate or use AI artwork. The default dark appearance is a plain black background; users may upload their own backgrounds. Commit messages use Conventional Commits with a scope: feat(serendipity): ..., fix(sanctuary): ..., chore(infra): ....

Serendipity is the Next.js/Tailwind/shadcn website, Sanctuary is the Rust API, Resonance is the client protocol library, Kaleidoscope is the reversible connection assistant. Persist data in PostgreSQL and media volumes. Enforce permissions on the server as well as hiding unavailable UI.

Use dynamic SQLx queries so builds do not require a live database. Secrets come from deployment environment. Test security-sensitive parsing and authorization. Preserve other contributors' changes.

Build this child through the Illyaverse parent checkout unless its README states otherwise. Preserve the parent workspace lockfiles, shared crates and migrations; do not infer standalone deployment from a separate Git repository.
