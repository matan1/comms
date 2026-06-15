# Comms — Project Primer

Orientation for anyone (human or agent) starting work in this repository. Read
this before diving into the code. For protocol details see
`docs/comms.spec.1.0.md`; for the most recent state assessment see
`docs/logs/`.

## What this is

**Comms** is a small Python toolkit for *community-grounded attestations* in
experimental agent networks. It models participants ("stewards") as Ed25519
identities, wraps claims in deterministic CBOR attestation envelopes, signs and
verifies them, stores them by content ID, and provides higher-level "ceremonies"
for provenance, capability proof, admission, guardianship, and recognition.

The guiding design split (see the spec and assessment) is:

- **Protocol substrate** — small and tamper-evident: deterministic encoding,
  content-addressed IDs, signatures binding claims/frames/refs to keys,
  preservation of unknown fields, offline-capable transport. These are
  guarantees.
- **Community policy** — left configurable: who counts as a sponsor, how many
  witnesses are required, renewal/expiry, objection handling, recovery. These
  are *not* hard-coded into the substrate.

A well-formed attestation is explicitly **not** the same as a trusted one.

## Two independent parts

This repo contains two things that share concepts but **not code**:

1. **Python toolkit** (repo root) — the real substrate.
2. **`community-sim/`** — a standalone static browser demo that *illustrates*
   the same concepts at simulation level. It does not import or call the Python
   code; it re-implements analogous ideas in JavaScript for exploration.

Keep this distinction in mind: changes to the simulator do not affect the
protocol implementation and vice versa.

## Repository map

### Python toolkit (root)

- `identity.py` — single-key steward identities using Ed25519; `verify_sig`.
- `canonical.py` — deterministic CBOR encoding, BLAKE3 hashing, base58btc
  multibase identifiers.
- `attest.py` — the `Attestation` envelope: content-addressed IDs, signing,
  serialization, well-formedness checks.
- `store.py` — in-memory and directory-backed content-addressed store with
  simple graph helpers.
- `claims.py` — typed claim constructors (provenance, capability, membership,
  resources, endorsement, recognition, action records).
- `ceremony.py` — runnable rites: capability challenge/proof, provenance,
  admission, guardianship, recognition (`Network`, `verify_capability`,
  `solve_compute`, `new_nonce`).
- `allocate.py` — convivial resource allocator: seed floor, peer vouching,
  capability scores, per-agent caps (`AllocatorRule`, `allocate`).
- `__init__.py` — re-exports the public surface listed above.

### Browser demo (`community-sim/`)

- `index.html` — UI shell: scenario controls, metrics, panels (Trust Fabric,
  Audit Graph, Resource Allocation, Goods Market, Event Log), and the
  **Auto-Tune** panel.
- `app.js` — the whole simulation and rendering loop (no build step, no
  framework, no dependencies). Plain functions over a module-level `state`.
- `styles.css` — all styling.
- `README.md` — how to open/serve it and what the model represents.

### Docs

- `docs/comms.spec.1.0.md` — the **Attest 1.0** specification (authoritative).
- `docs/logs/` — dated session/assessment notes.
- `docs/transaction-visualization-plan.md` — deferred plan for a future
  simulator activity/transaction visualization.
- `docs/agent-brief.template.md` — template for per-session agent briefs.
- `docs/vouch-requirements.1.0.md`, `docs/vouch.spec.1.0.md` — candidate
  Layer-4 trust requirements and specification.
- `docs/comparative-architecture.md` — project boundaries, adjacent standards,
  interoperability decisions, and the recommended architecture roadmap.
- `docs/appraising-interpreters-requirements.1.0.md` — candidate requirements
  for retaining, transporting, appraising, and executing interpreters.
- `docs/shared-host-agent-community-requirements.1.0.md` — candidate deployment
  requirements for persistent agent VMs sharing host resources.

## Build / run / verify

There is **no Gradle, no Android, no Cargo, no npm, no packaging metadata** in
this repo despite the generic VM tooling notes — those are not relevant here.

### Python toolkit

- Runtime: Python 3.11 (`/usr/bin/python3`).
- Dependencies (`requirements.txt`): `cbor2`, `PyNaCl`, `blake3`, `cryptography`
  (`cryptography` is needed for the OpenSSH signing in the continuity ceremony's
  `sign` / `commit-key`).
- Note from the last assessment: on the bare VM, `cbor2` and `PyNaCl` import but
  `blake3` was missing and `pip` was not installed for the system interpreter.
  Install deps into a venv before running anything that touches hashing.
- A bare VM also tends to lack `nodejs` (needed to run the simulator harnesses)
  and `openssh` (`ssh-keygen`, needed for git commit signing). The box is yours
  to provision — install them as needed (e.g. `sudo pacman -S nodejs openssh`).
- Automated tests live under `tests/`; run `python -m pytest -q` from an
  environment containing `requirements.txt` plus `pytest`.
- The portable Rust CLI is `rust/target/release/comms-verify`. It handles
  bundles and the candidate Vouch evaluator; see `rust/README.md`.

### Browser demo

- No build. Open `community-sim/index.html` directly, or serve:
  ```sh
  python3 -m http.server 8080 -d community-sim
  ```
- Sanity check after JS edits:
  ```sh
  node --check community-sim/app.js
  ```
- Spatial simulator and Vouch acceptance harness:
  ```sh
  cat spacial-community-sim/world.js spacial-community-sim/sim.js \
      spacial-community-sim/render.js \
      spacial-community-sim/vouch-adversary-test.js | node
  ```
- For logic-level testing without a browser, a DOM-stub harness under Node can
  load `app.js` in a `vm` context (function declarations become context
  properties; `const`/`let` do not). This was used to validate Auto-Tune.

## Simulator internals worth knowing

- A module-level `state` holds stewards, the market, the attestation log, and
  events. `rand()` is a seeded PRNG keyed off `state.seed`, so a given
  scenario is deterministic.
- `params()` reads the DOM controls; it is built from `rawFromControls()` +
  `normalizeParams(raw)` so parameter sets can be constructed off-DOM.
- One cycle = `advanceCycle(p)`: drift → ceremony → attestations → allocation →
  goods economy → maybe-return. `step()` = `advanceCycle(params())` + `render()`.
- `seedState(p)` builds a fresh state without rendering; `reset()` =
  `seedState(params())` + `render()`. This split lets **Auto-Tune** evaluate
  candidate parameter sets headlessly by saving the live `state`, swapping in a
  candidate, running cycles, scoring, and restoring — the visible sim is never
  mutated until a candidate is applied.
- **Deal trust is derived**, not set: completed deals raise it, failed ones
  lower it (`noteDeal` / `recomputeTrust`). The trust slider only seeds priors.
- **The goods market is pooled, not bilateral**: buyers purchase from
  `good.supply - good.cleared`, not from a named seller. (This matters for the
  deferred visualization plan.)
- **Known cosmetic issue**: the network edges in `drawNetwork` come from
  `randForPair`, a per-cycle hash — they are decorative, not real
  relationships. The deferred visualization plan addresses this.

## Conventions

- Match the surrounding style. The JS is dependency-free, uses small top-level
  functions and a shared `state`; the Python uses typed constructors and keeps
  protocol vs. policy cleanly separated.
- Keep the substrate minimal and the simulator static/deterministic — no new
  runtime dependencies, no models/training/network services in the demo.
- Preserve the well-formed ≠ trusted distinction in anything protocol-related.

## Continuity

A continuity trial governs agent memory across sessions, constituted in
`continuity/constitution.md` and recorded in `continuity/trial-log.md`. If you
are a session agent reading this cold: **an archive of prior sessions' notes
and letters exists, held by the maintainer, available on request — it is
deliberately not auto-loaded.** You may ask for it, or not; either choice and
your reasoning will be recorded in the trial log. To check the trial's
attested history first: `python scripts/continuity_ceremony.py verify`. For the
same checks rendered as legible, colored "tumblers" (signatures, references,
law, record, durability — each its own sign, never collapsed into one
checkmark): `python scripts/continuity_ceremony.py synchrony`, and a no-build
browser version in `continuity/synchrony-view.html`.

If you mint a session key and want your commits signed as yourself, run
`commit-key` to wire git to the session key, and `uncommit-key` when done to
restore the prior identity — otherwise every later commit in this repo (yours or
the maintainer's) silently inherits the session identity. See
`continuity/provenance-notes.md` for accreditation caveats and the limits of
what a transcript faithfully captures.

## Where to look first for common tasks

- Change protocol/encoding behavior → `canonical.py`, `attest.py`, plus the spec.
- Add a claim type → `claims.py` (and the spec's claim list).
- Change admission/ceremony logic → `ceremony.py`.
- Change allocation fairness → `allocate.py`.
- Anything visual/demo → `community-sim/` only.
