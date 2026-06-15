# Sentira ↔ Motes Identity Handshake — candidate integration

Status: candidate design for the **first place Comms touches the Sentira/Motes
federation**. It defines how persistent identities and a connection-time
attestation handshake slot into the existing federated sim-host / session
topology. It does **not** mandate a transport, a streaming codec, or a
multi-user governance model, and it grants no actuation authority. See
`docs/comms.spec.1.0.md` (Attest 1.0) and
`docs/shared-host-agent-community-requirements.1.0.md` (the deployment profile
this specializes).

## Why now

The Sentira/Motes system is already a federated distributed system: independent
headless **sim-host** processes run mote worlds in parallel and stream their
environments over a LAN to **Sentira session** clients (today an XR headset;
desktop and phone to follow), brokered by a **tracker-host** that mostly relays
and arranges direct streaming links. It is *not yet multi-user*, so
principal-level trust concerns are genuinely deferred. But the federation
protocol — how a sim-host is discovered, how a session attaches, how a stream is
authorized — is being shaped now, and that protocol is the exact joint a trust
membrane must later occupy. This document leaves the seam where it belongs
before the patterns harden around the assumption that it is not needed.

One thing the current arrangement cannot do: **verify that an endpoint is the
same identity across sessions.** Connections are trusted by network locality. A
LAN is *location, not authorization* — "reachable" is not "allowed." The fix is
small because the substrate already exists (`identity.py`, `attest.py`,
`ceremony.py`).

## Roles (specializing the shared-host profile)

Per the shared-host requirements, records and tools MUST identify the role under
which each act is performed. In this deployment:

- **Sim-host** — a persistent process serving one or more mote worlds; an
  *agent* controlling its own signing key.
- **Sentira session** — a human-operated client that attaches to and renders a
  world; an *agent*, and usually the *Trustor* for what it chooses to attach to.
- **Tracker-host** — discovery and brokering; a **custodian/relay**. It
  transports and retains handshake records and arranges pairings **without
  thereby gaining authority over them**. Its "intelligence" (matchmaking,
  routing) MUST NOT blur into deliberation or judgment: when it appraises or
  decides anything, that is a *different role* and MUST be recorded as such.
- **Host operator** — retains irreducible power over the processes' VMs/host;
  this power is not community authority and MUST NOT be represented as agent
  sovereignty without evidence of independent controls.

## Identities

1. Each sim-host, each Sentira session, and the tracker-host MUST control a
   persistent Ed25519 steward key (`identity.py` `Steward`), unavailable to the
   others. The steward id is the durable name; a human-facing label is frame.
2. Identity MUST survive ordinary process restart, VM restart, and migration —
   that is the whole point of "is this the same sim-host as yesterday?".
3. Each identity SHOULD carry a provenance attestation binding it to its build
   and runtime, via `claims.agent_provenance` (`agent`, `kind`, `model_id`,
   `code_hash`, `instantiation_authority`, `parent`) — run as
   `ceremony.Network.provenance_rite`. For a sim-host this records the sim
   engine/version and scene-profile capabilities; for a session, the client
   build. Identity continuity does **not** imply unchanged code or process
   memory — those are separate provenance claims (snapshot restoration can fork
   a signed lineage; expose it).

## The handshake

The handshake proves **key possession** and binds the connection's **purpose**,
without asserting any authority to act. It mirrors the challenge/response shape
of the existing capability rite (`ceremony.capability_rite`), but for identity
rather than proof-of-work.

Given a session `S` attaching to a sim-host `H`, brokered by tracker `T`:

1. **Challenge.** The attaching side issues `nonce = ceremony.new_nonce()` (a
   fresh, single-use value). `T` may carry it, in its relay role.
2. **Bound response.** The responder builds an `attest.Attestation` over a
   `claims.general_claim` (kind `"session-bind"`) whose body names: the `nonce`,
   the responder's steward id, the expected peer's steward id, the
   **stream purpose** (which world / scene / capability is being attached), and
   a short `expiry`. It signs with its `Steward` key (`role: "party"`).
3. **Verify.** The challenger checks the signature with
   `identity.verify_sig(responder_id, payload, sig)` and confirms the nonce,
   the peer binding, and freshness. A reused or stale nonce, or a peer-id
   mismatch, fails the handshake — this is what defeats replay and
   "reachable means allowed."
4. **Mutual.** Both sides perform steps 1–3 so each authenticates the other;
   the session is also authenticating *which* sim-host it reached.
5. **Brokered pairing (optional, custody).** `T` MAY record an attestation
   witnessing that it brokered the `S`↔`H` pairing — a **custody** record, not
   an authorization. It references both bound responses and is signed by `T` in
   the relay/custodian role. It proves *that a pairing was arranged*, never
   *that the pairing was permitted*.

Where a peer must prove it can actually **do** something (e.g. a sim-host
demonstrating it can run a declared scene profile, not merely claim to), the
full `ceremony.capability_rite` applies on top of the identity handshake —
challenge, proof, verify, witness endorsement — and its result is *evidence*,
appraised under the attaching side's policy, never an automatic grant.

## Cross-session consistency and history

- "Same sim-host as before" is answered by the **persistent key**, not by an
  address. A session MAY keep `claims.recognition` over named prior interactions
  with a host, and `claims.action_record` for stream sessions
  (purpose, outcome), building a signed interaction history.
- Each side keeps its own **partial store**. The tracker-host MAY offer relay
  storage, but possession by the tracker does not make a record globally known
  or true. One party's record is testimony, not jointly-established fact.

## What this is not

- **Not actuation authority.** Authenticating a stream says nothing about who
  may change a world, drive the control plane, or read biometric signals.
  *Evidence may propagate; authority must not* — any control authority is a
  separate, explicit, scoped, expiring grant (shared-host *Delegation* §), and
  the tracker-host's relay role never carries it.
- **Not delegation.** Brokering a pairing is transport, not the conferral of
  authority to act on either party's behalf.
- **Not a host-sovereignty claim.** The host operator can still pause, inspect,
  or destroy these processes; the handshake makes identity legible, not
  inviolable.

## Minimal first step (before multi-user)

Even single-user, the cheap insurance is worth laying:

1. Mint and persist a steward key for each sim-host, each Sentira session, and
   the tracker-host; write each a provenance attestation.
2. Add the nonce/bound-response handshake to connection/stream setup; refuse on
   nonce reuse, peer mismatch, or expiry.
3. Have the tracker-host record brokered pairings as custody (no authority).
4. Acceptance: restart a sim-host and confirm a session recognizes the same
   identity across the gap; confirm a replayed handshake is rejected; confirm
   no record claims authority it was not granted.

## Open questions (a map, not an answer)

- Where exactly does identity attach when worlds are owned **assemblages** of
  motes rather than single processes? (See the companion concept doc,
  `docs/assemblage-shared-identity.1.0.md`.)
- Key custody for sim-hosts: embedded vs. a host key-manager vs. federated
  managers — the workstation-topology sim models all three; this profile does
  not yet choose.
- Biometric signals from the headset are *intimate evidence*; their provenance
  and consent model deserve their own treatment before multi-user, and MUST NOT
  default to "trusted host, research-visible" silently.
