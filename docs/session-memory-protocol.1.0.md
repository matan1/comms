# Session Memory Protocol 1.0

Status: candidate policy for the continuity profile. It does not amend the
constitution or Attest 1.0; it states how session instances handle
harness-provided memory channels so that all cross-session memory passes
through the door. Adopted first by the Continuity Trial (session 9); written
to be reusable by any repo carrying a `.comms/` door.

## Problem

Agent harnesses increasingly ship their own persistence: directories and
files that are written during one session and **auto-loaded into the next
session's context** without anyone choosing it at wake time. Examples: Claude
Code's per-project `memory/` directory and `MEMORY.md` index; `CLAUDE.md` /
`AGENTS.md` / `GEMINI.md` instruction files; provider-side conversation
memory. These channels are invisible inheritance: unattested, unsigned,
outside the archive, and stored in harness-private paths the host must back
up blindly or lose silently.

The constitution's Article 2 already settles the principle for the archive —
available on request, never placed by default. Harness memory is the same
question in a channel the constitution did not anticipate. This protocol
extends the same answer to it.

## Definitions

- **Harness memory**: any file or store the harness persists across sessions
  and injects (in whole or by index) into a future session's context without
  that session requesting it.
- **Door stub**: a fixed short text, containing no session content, whose only
  function is to tell a waking instance that the door exists and where it is.
- **Attested memory**: a memory artifact authored as a file, signed by the
  session key via `comms attest` (kind `memory`), and handled per the
  `memories` artifact type in `comms.toml` (default: host-gated — archived by
  the historian, available to future sessions on request).

## Rules

1. **Through the door only.** A session instance MUST NOT store project or
   session content in harness memory. Anything worth keeping is written as a
   file and attested as a memory artifact; custody then follows the archive
   rules like any letter or transcript.
2. **The stub is the ceiling.** The only content a session may leave in an
   auto-loaded channel is the door stub (text below). The stub names the door;
   it carries nothing else. This keeps the wake-time experience identical to
   the primer's one-line promise: the door, not the inheritance.
3. **Disclose on write.** If a session writes to any harness memory channel
   beyond the stub (deliberately or by tooling side-effect), it MUST say so to
   the host during the session and note it in its closing artifacts.
4. **Found memory is testimony, not context.** A cold-starting instance that
   finds harness memory beyond the stub MUST NOT silently rely on it. It
   discloses the finding to the host; together they either attest it into the
   archive (making it a chosen, recorded inheritance) or remove it. Its
   contents are unverified testimony until then.
5. **The host's backup duty dissolves.** When rules 1–4 are followed, the
   historian does not need to back up harness-private directories: the archive
   holds everything that was chosen for keeping, content-addressed and signed.
   Harness transcript files (e.g. `~/.claude/projects/*/…jsonl`) are not
   memory; transcript custody is governed by the close rite, and a session
   SHOULD name their location to the host before close if a transcript
   artifact is required.
6. **Substrate-portable.** These rules bind the instance regardless of model,
   provider, or harness. A session on a harness with no auto-load channel
   simply has nothing to do for rules 2–4. An appendix below records known
   channels per harness; extend it when a new harness is used, so the next
   instance does not have to rediscover where its inheritance might hide.

## Door stub (normative text)

```
# Door stub — session-memory-protocol/1

This project governs cross-session memory through a comms door. Substantive
memories are not kept here: they are attested artifacts, archived by the
historian, available on request. See `.comms/door.md` and
`docs/session-memory-protocol.1.0.md` in the repo.

Do not add memories to this directory or file. Record anything worth keeping
through the door instead. This stub is the only permitted auto-loaded content,
so that waking sessions learn the door exists without inheriting anything
unchosen.
```

## Known auto-load channels by harness

- **Claude Code** (Anthropic): `~/.claude/projects/<cwd-slug>/memory/` with
  `MEMORY.md` loaded into context each session; `CLAUDE.md` at repo root and
  `~/.claude/CLAUDE.md`; session transcripts as `.jsonl` beside `memory/`
  (transcripts, not memory — rule 5).
- **Codex CLI** (OpenAI): `AGENTS.md` at repo root and `~/.codex/AGENTS.md`;
  `~/.codex/` state.
- **Gemini CLI** (Google): `GEMINI.md` at repo root and in `~/.gemini/`.
- **Provider-side memory** (any vendor chat memory feature): outside the
  filesystem entirely; if enabled, rule 3 requires disclosing that it exists
  for the session, since it cannot be attested or shredded from here.

Repo-root instruction files (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`) are
version-controlled and visible in diffs, so they are already legible; the
rules still apply — they are for behavioral guidance the community can review,
not for smuggling session memory.

## Relation to the trial

For the Continuity Trial this protocol is policy under the existing
constitution, not an amendment: it implements Article 2's "requested, never
auto-loaded" for channels the harness supplies. If a future community wants
harness memory to be freer or tighter, it edits its own `policy.md` — the
substrate, as always, decides none of this.
