# comms-core + comms-verify — the portable sneakernet kit

A small Rust reference for **Comms Attest 1.0** (+ Amendment A1) and **Steward
1.0**, plus a single static binary, `comms-verify`, that does the whole offline
bundle loop on a machine with **no Python and no Cargo**:

```
pack → seal → inspect → verify → extract
```

The crate is the protocol substrate only. Every verify function is layer 2/3
per A1.4 — *verified* (the signatures check) and *resolvable* (the chain is
reachable). A pass means **the math holds, not "trust this."** Trust is a
community/human judgment and lives nowhere in here by design.

## Build

```sh
cargo build --release      # -> target/release/comms-verify
cargo test                 # golden conformance vs the Python-generated vectors
```

Dependencies are pure-Rust (ed25519-dalek, blake3, bs58, serde_json), so the
binary is statically linkable and travels well.

## Commands

```
comms-verify <command> [args]

init    [dir] [--profile P]       install the .comms/ harness door into a repo
        [--dry-run] [--force]     (profiles: default, continuity; default dir: .)
attest  --key <k.json> --about S --kind S --body <file|->   author + sign a
        [--media-type T] [--language L] [--community C]      general-claim/1
        [--occasion O] [--role R] [--support ID]... [--out F] (default <id>.cbor)
status  [dir] [--json]            where you are in the rite + the next step
next    [dir] [--rite N]          perform the next pending step of a rite
        [--body F] [--about S] [--kind K]  (attest steps need --body)
verify  <bundle>                 check the A1.8 integrity seal (default; bare
                                 path also works: `comms-verify b.cbor`)
inspect <bundle> [--json]        verify EVERY member on its own terms
seal    <bundle> --key <k.json> [--out <p>] [--description S]
                                 [--created-at T] [--issued-at T] [--signed-at T]
pack    --out <bundle> [<att.cbor|dir>...] [--media F]...
                                 [--seal --key <k.json>] [--description S] [--*-at T]
extract <bundle> --out <dir>     write each member <id>.cbor and media blob to disk
mint    --out <k.json> [--label L]   generate a steward key for sealing
```

### init — install the door

`comms-verify init` writes the embeddable harness boundary (`docs/embeddable-harness-v1.md`)
into a repo: a `.comms/` directory holding `comms.toml`, `door.md`, `policy.md`,
a public `store/`, `hooks/`, and a directory for each artifact type the profile
declares. It is the step Codex's install flow opens with.

```sh
comms-verify init                      # default profile, into the current repo
comms-verify init path/to/repo --profile continuity
comms-verify init --dry-run            # preview the plan, touch nothing
comms-verify init --force              # rewrite door files from the template
```

- **`default`** — minimal door: config, `policy.md`, `store/`, `hooks/`.
- **`continuity`** — adds the session-rite shape: `letters/`, `transcripts/`,
  `memories/`, `pending/`, and the matching `[artifact_types.*]` declarations.

Installation is idempotent: existing files are kept (a local edit survives a
re-`init`) unless `--force` is given. `init` only writes the door — it never
imports an archive, mints a key, or decides trust. Built-in profiles are the
single source of truth for both the `comms.toml` text and the directories
created; merging user/repo/archive config layers per the design's precedence
chain is a later slab.

### attest — author a primary attestation

`pack` and `seal` move attestations that already exist; `attest` is how you
create one. It authors a `general-claim/1` (a letter, transcript, memory, or any
statement) from a content file and signs it with a steward key, writing the
`<id>.cbor` envelope. It is the create-side sibling of the seal builder, and it
is byte-identical to the Python reference for the same inputs (a golden test
pins this against the published vector).

```sh
comms-verify mint   --out steward.json --label me
comms-verify attest --key steward.json --about "session 8 letter" --kind testimony \
                    --media-type text/markdown --body letter.md --out letter.cbor
comms-verify pack   --out session.bundle letter.cbor --seal --key steward.json
```

`--body -` reads stdin. `--kind` follows the spec set (observation, synthesis,
prediction, testimony, translation, other); `--media-type` defaults to
`text/plain;charset=utf-8`; `--support ID` (repeatable) adds claim-level
supporting ids. A signed attestation is layer-1 — well-formed and signed — never
a trust judgment.

`pack` now accepts a media-only bundle (no `.cbor` members) and warns, rather
than silently packing zero members, when a directory contributes no
attestations.

### status / next — drive a rite from comms.toml

The harness workflow is config-driven, not hardcoded. A profile's `comms.toml`
declares each **rite** as an ordered list of `"verb target"` steps; the binary
knows how to perform each verb (`mint`, `attest`, `seal`, `shred`) and how to
detect whether it has been done, so a profile defines its own flow without new
code. `status` reports position; `next` performs the next pending step.

```toml
[rites.open]
steps = ["mint session", "attest entry"]

[rites.close]
steps = ["attest transcript", "seal store", "shred session"]
```

```sh
comms-verify status                         # where am I? what's next?
comms-verify next --rite open               # mint the session key
comms-verify next --rite open --body e.md   # attest the opening entry
comms-verify next --rite close --body t.md  # attest the transcript
comms-verify next --rite close              # seal the store -> close.bundle
comms-verify next --rite close              # shred the session key
```

A rite is treated as an ordered sequence (a step is done only if it and all
prior steps are), so a trailing `shred` does not read as done before the session
has begun. Targets are an artifact-type name or a built-in noun (`session`, the
key at `.comms/<target>.key`; `store`, the `.comms/store` dir to seal). `status`
prints the exact next command, and `--json` gives a successor agent a
machine-readable "current state + next action." A small dependency-free reader
handles the `comms.toml` subset (dotted tables, strings, bools, string arrays)
so the binary stays a single static file.

### verify vs inspect

`verify` checks the **A1.8 seal**: the seal's own signature, the bundle hash,
and that the member *set* is exactly as sealed (no drops, no smuggled
additions). It does **not** check whether each member is itself validly signed.

`inspect` closes that: it verifies **each member's own signatures** — personal
(`ed25519`) and community/threshold (`ed25519-set/1`, resolved through the
keyset chain *within the bundle*) — lists each ref and whether it resolves
in-bundle, checks media content hashes, and reports the seal. It exits non-zero
if anything fails. A corrupted member signature leaves the member *set* intact,
so `verify` still passes while `inspect` catches it — run `inspect` when you
care about authenticity, not just completeness.

## Vouch candidate

The binary also carries the candidate Layer-4 reference evaluator. Unlike the
commands above, `vouch` returns a viewer- and policy-relative judgment:

```sh
comms-verify vouch evidence.bundle \
  --policy comms.attest:z... \
  --subject comms.steward:z... \
  --purpose admission \
  --as-of 2026-06-14T12:00:00Z \
  --json
```

Add `--receipt-out judgment.cbor --key steward.json` to emit an optional signed
`vouch-judgment/1` receipt. The receipt preserves the query, policy, exact
store-view digest, outcome, and evidence IDs; it does not prove the judgment
was wise. See `docs/vouch.spec.1.0.md`.

## Keys

`seal`/`pack --seal` sign with a **steward key file**, the same JSON the Python
toolkit writes (`identity.py:Steward.save`):

```json
{ "seed_b58": "<base58btc of the 32-byte Ed25519 seed>", "label": "..." }
```

`mint` creates one (mode 0600). Reproducible bundles: pass explicit
`--created-at/--issued-at/--signed-at` (RFC 3339 UTC, `Z`, second precision);
they default to *now*. Signing the historian's durable **OpenSSH** key directly
is a deliberate non-goal here (it would add an `ssh-key` dependency) — mint a
steward key, or sign in Python.

## Cross-implementation contract

The golden tests (`golden.rs`, vectors under `data/`) pin agreement with the
Python reference byte-for-byte in **both** directions:

- Rust parses and verifies the Python-produced bundle/seal vectors.
- Rust `build_seal` re-creates the Python seal **byte-identically** (same
  attestation id, same deterministic Ed25519 signature), and an assembled
  container reproduces the exact Python bundle bytes.

So a bundle sealed by either implementation verifies under the other.

## `cargo xtask` — recurring operations

One cargo-native entrypoint for the whole workflow (run from this `rust/` dir).
It forwards ceremony ops to the Python continuity ceremony (venv + PYTHONPATH
wired in) and bundle ops to `comms-verify`:

```sh
cargo xtask status                 # where am I in the session ceremony?
cargo xtask open --auto-derive ... # mint + stage a new session log entry
cargo xtask close --transcript F   # the instance's closing rite
cargo xtask sign --key <ssh-key>   # historian countersigns
cargo xtask finalize               # seal signed items into the store
cargo xtask verify                 # walk + verify the store (the door)
cargo xtask log [--session-num N]  # render the trial-log.md entry from the store
cargo xtask anchor [key] [out]     # pack+seal+verify the store into one bundle
cargo xtask bundle <args>          # passthrough to comms-verify (verify/inspect/...)
cargo xtask test                   # cargo test + pytest
```

## Worked example — the continuity store

```sh
B=target/release/comms-verify
$B mint  --out demo.steward --label demo
$B pack  --out continuity.bundle ../continuity/store --seal --key demo.steward \
         --description "continuity trial store"
$B verify  continuity.bundle     # seal: ok
$B inspect continuity.bundle     # each session attestation verifies; refs resolve
```

This packages the trial's own attestations (the genesis transcript record, the
constitution `rule/1`, each session's faithfulness/custody records and
endorsements) into one sealed file that anyone can verify offline — the
portability Article 5 of the constitution asks for.
