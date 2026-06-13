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

verify  <bundle>                 check the A1.8 integrity seal (default; bare
                                 path also works: `comms-verify b.cbor`)
inspect <bundle> [--json]        verify EVERY member on its own terms
seal    <bundle> --key <k.json> [--out <p>] [--description S]
                                 [--created-at T] [--issued-at T] [--signed-at T]
pack    --out <bundle> <att.cbor|dir>... [--media F]...
                                 [--seal --key <k.json>] [--description S] [--*-at T]
extract <bundle> --out <dir>     write each member <id>.cbor and media blob to disk
mint    --out <k.json> [--label L]   generate a steward key for sealing
```

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
