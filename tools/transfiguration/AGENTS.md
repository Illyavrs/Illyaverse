# Illyaverse child-project policy

# Illyaverse development

All implementation is original. Reference projects may inform protocol behavior and architecture; do not copy their source. Prefer primary upstream documentation and record important protocol assumptions in docs/protocol-compatibility.md.

Keep honest capability states: implemented, integration-tested, client-tested, and pending are different. Never mark stock stable/lazer connectivity complete without real client verification. Do not render invented activity as live server data.

Do not generate or use AI artwork. The default dark appearance is a plain black background; users may upload their own backgrounds. Commit messages use Conventional Commits with a scope: feat(serendipity): ..., fix(sanctuary): ..., chore(infra): ....

Serendipity is the Next.js/Tailwind/shadcn website, Sanctuary is the Rust API, Resonance is the client protocol library, Kaleidoscope is the reversible connection assistant. Persist data in PostgreSQL and media volumes. Enforce permissions on the server as well as hiding unavailable UI.

Use dynamic SQLx queries so builds do not require a live database. Secrets come from deployment environment. Test security-sensitive parsing and authorization. Preserve other contributors' changes.

Build this child through the Illyaverse parent checkout unless its README states otherwise. Preserve the parent workspace lockfiles, shared crates and migrations; do not infer standalone deployment from a separate Git repository.
