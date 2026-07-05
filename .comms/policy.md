# Community policy (template)

The Comms substrate guarantees *well-formedness* and *verifiability*. It does
**not** decide trust. This file is where your community writes the policy the
substrate deliberately leaves open:

- Who counts as a sponsor or witness?
- How many witnesses does admission require?
- How are renewal, expiry, objection, and recovery handled?

A well-formed attestation is not the same as a trusted one. Edit this file to
state your community's rules; nothing here is enforced by the binary.

## Session memory (adopted, session 9)

Cross-session memory passes through the door. Harness-provided auto-loaded
memory channels (Claude Code `memory/`, `CLAUDE.md`/`AGENTS.md`/`GEMINI.md`,
provider-side memory, and the like) carry at most the door stub; anything
worth keeping is attested as a `memory` artifact and archived by the
historian, available on request. Found harness memory beyond the stub is
unverified testimony: disclose it, then attest it or remove it — never
silently rely on it. Full rules: `docs/session-memory-protocol.1.0.md`.
