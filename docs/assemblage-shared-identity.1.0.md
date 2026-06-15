# Assemblage Shared Identity — concept note

Status: **concept, not specification.** It explores how an *assemblage* — an
owned environment of cooperating agents, whether bacterium-simple **motes** or
heavier **LLM-based** agents — might share a single signing identity while
distributing agency internally in flexible, economically useful ways. It
proposes no normative claim schema and decides nothing; it maps the design space
and keeps the open questions open. Grounds: `identity.py`, `claims.py`,
`comms/allocate.py`, and `docs/shared-host-agent-community-requirements.1.0.md`.

## The tension

Comms gives an *agent* one persistent signing key (shared-host *Agent Identity*).
An assemblage is many participants that want to **answer to the outside world as
one** — one reputation, one set of grants, one thing a counterparty trusts —
while **inside**, agency is plural, churning, and unequal. A swarm of motes has
no use for a passport per cell; a team of LLM agents may want individual
attribution. Both want a single external identity over a distributed interior.

Cartographer's distinction is the hinge: **identity lineage and lifecycle are
modeled separately.** One identity may persist across many transient
expressions; one VM may host an assemblage concurrently. So "how many agents"
and "how many identities" are different questions, and the interior can change
shape without the exterior identity blinking. Biologically: the organism (or the
colony) is the agent; cells do not each sign; a cell's *behavior* is the
evidence, and the *bounded thing* around it is the identity.

## Candidate models (options, not a decision)

### A. Monolithic shared steward
The assemblage is one `Steward`; members act through it. Simplest externally,
weakest internally: whoever holds the key *is* the assemblage, so the key alone
cannot express internal scope or attribution. Workable only when the interior is
genuinely undifferentiated (a mote swarm where no single mote's act needs to be
distinguished) — and even then the key custody question (embedded vs. a
key-manager VM vs. federated managers; see the workstation-topology sim) decides
who can speak as the whole.

### B. Lineage identity with endorsed sub-identities
The assemblage steward is the external identity; each differentiated member
holds its own `Steward` key, **bound** to the assemblage by
`claims.membership_binding` (steward, community=assemblage, role, capabilities,
effective period) and/or `claims.endorsement`. The world sees the assemblage;
internally, each member's acts remain attributable to its own key, and the
assemblage vouches for membership. Fits LLM-agent assemblages well, and the mote
case degrades gracefully: where members are too simple/numerous to each sign,
the **owned environment** is the bound identity and mote *functions* emit claims
under it, without minting a key per mote.

### C. Threshold / capability-scoped agency
Acting *as the assemblage externally* is gated — an m-of-n threshold over member
keys, or a standing policy the assemblage's own controller executes — while
*internally* each member receives an explicit, scoped, expiring capability or
resource grant. This keeps the high-authority external voice deliberate and the
low-authority internal work fluid.

These compose: a real assemblage might be **B for attribution + C for
authority** — members hold sub-keys bound to a lineage identity, and the right
to speak or spend *as the whole* is threshold- or policy-gated.

## Distributing agency economically

"Agency," made concrete, is a **bounded claim on shared resources** — and Comms
already has the machinery, which is what makes shared identity *useful* rather
than merely tidy.

- The assemblage holds shared **resource grants/leases** (shared-host *Resource
  Grants*): scoped, expiring, with explicit delegation rules.
- Members draw on them under **delegation that attenuates, never expands**
  (shared-host *Delegation*): a delegated grant references its parent, stays
  within its resource/quantity/period/domain, and resolves in a chain back to
  authority the host controller recognizes. Social trust and membership do
  **not** by themselves confer the authority to allocate onward.
- Internal distribution can run through the **convivial allocator**
  (`comms/allocate.py`, `AllocatorRule`, `allocate`): a **seed floor**
  guarantees every member a baseline share regardless of request ("everyone gets
  a seed"), while a **per-agent cap** prevents any one member from capturing the
  assemblage; the merit pool is split by *need × capability × community vouch*
  (and you cannot vouch for yourself). That is a ready-made, inspectable way for
  a shared-identity assemblage to divide its resources flexibly — egalitarian at
  the floor, meritocratic and vouch-weighted above it, concentration-resistant
  at the cap.

So the assemblage answers externally as one steward, holds resources as one
grantee, and **internally distributes agency as attenuating, capped, seed-floored
grants** — flexible, and economically legible.

## The membrane, restated for assemblages

- **Evidence may propagate:** a member's completed work, capability proofs, and
  interaction history are evidence for the assemblage's reputation, and the
  assemblage's standing is context for its members.
- **Authority must not:** no member binds the assemblage to *new* authority, and
  the assemblage confers no authority on a member, except by an explicit, scoped,
  attenuating grant. Histories stay purpose-specific — successful storage work
  is not evidence of governance authority or truthful reporting.

## Lifecycle ≠ identity (keep them apart)

The assemblage identity persists while members join, leave, suspend, or are
replaced. Distinct events deserve distinct records: member admission and exit;
suspension/quarantine; **snapshot restoration (which can create a signed fork
from earlier state — preserve both histories and expose the rollback)**; key
destruction; migration; retirement. Key rotation or recovery for the shared
identity MUST preserve prior history and expose any competing succession claims,
not silently overwrite the past.

## Open questions (deliberately unresolved)

- **Authority gate:** threshold key vs. policy-executed standing grants vs. a
  designated speaker — which suits motes, which suits LLM agents, and can they
  coexist in one assemblage?
- **Revoking a member's agency** without erasing its historical attestations,
  and propagating that revocation to the host controller as an enforcement
  receipt.
- **Opaque interiors:** how does an external counterparty appraise an assemblage
  whose internal composition is private? Completeness claims must stay bounded
  ("complete for manifest X"), and "no known gaps" is not "complete."
- **Compromise of the shared key:** recovery that preserves history and surfaces
  competing succession, given that the shared key is a larger blast radius than
  an individual's.
- **Where identity attaches in the mote case** specifically — per-colony,
  per-owned-environment, or per-simulation-space — remains genuinely open (see
  `docs/sentira-motes-identity-handshake.1.0.md`); this note assumes only that
  *some* bounded thing carries the identity and mote functions attest beneath it.

This is a map of a place we have not built. Its job is to keep the signs legible
and the uncertainty honest, not to make the interface feel decisive.
