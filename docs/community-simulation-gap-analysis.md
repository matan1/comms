# Community Simulation Gap Analysis

Status: Session 12 (Sol) overview for future simulation teams. Read alongside
`community-simulation-next-tier-brief.md` and the existing simulator READMEs.

## Executive view

The current simulators demonstrate that viewer-relative appraisal over partial
stores produces meaningful community dynamics. Their chief blind spot is that
attestations generally appear already authored and available for gossip. They
model the circulation and appraisal of evidence more deeply than its creation,
authorization, custody, or decay.

The next research program should simulate the complete life of evidence:

```text
experience → proposed account → pending signature → appraisal →
clarification/defer/decline/sign → transport → partial possession →
policy-relative judgment → authority decision → enforcement →
challenge/disposition → custody/decay
```

## What should be retained

### Partial knowledge as ordinary state

Each villager owns a local store and learns in an order determined by presence,
travel, and gossip. This is one of the strongest features in the project. Do
not replace it with a global ledger merely to simplify multi-community work.

### Viewer-relative rendering

Selecting a villager recolors the world according to that viewer's evidence.
The interface demonstrates disagreement without requiring dishonesty. New
policy and archive views should preserve this technique.

### Deterministic research harnesses

Seeded headless runs permit policy comparison and regression testing. Keep the
logic/render split and add focused experiments before expanding another large
matrix.

### Spatial and resource costs

Travel cost makes information latency legible. The workstation adds scarce
accelerator resources, service failures, and host topology. Future evidence
transport, reviewer attention, storage, and enforcement should likewise have
cost rather than Boolean availability flags.

### Ground truth separated from participant view

Adversary presets are visible to researchers but hidden in a villager-relative
view. Preserve this separation and label every metric that consumes privileged
simulator knowledge.

## Blind spots

### Before signature

Records appear automatically after deals and ceremonies. There is no proposed
wording, requested signing role, provenance uncertainty, clarification,
decline, or attention budget. This makes signatures costless and removes the
possibility of legitimate refusal.

### Cryptographic and content state

Sim-level attestations are always structurally usable. Missing detached bodies,
unresolved refs, invalid signatures, unsupported interpreters, key compromise,
corruption, and retained mismatches are absent. Not every social scenario
needs real cryptography, but these distinct states should exist when evidence
lifecycle is the research subject.

### Heterogeneous law

Stores differ while appraisal policy is mostly shared. Comms permits two
Trustors to possess identical evidence and apply different, purpose-specific
laws. Policy plurality, succession, interpreter drift, and policy forks are
underrepresented.

### Authority and enforcement

Admission and resource use occur, but the complete chain from request through
grant, delegation, host acceptance, enforcement, and receipt is not modeled.
Practical host power and legitimate community authority can therefore be
described but not made to diverge through events.

### Custody and disclosure

Possession, custody, archive intake, minimal manifests, requested delivery,
grant, and reading are not separate states. The simulator cannot yet ask how a
custodian biases history through omission, delay, ordering, or threshold speech.

### Multi-community life

The farmstead is distant but belongs to the same community. There are no
independent laws, archives, community identities, recognized neighbors,
schisms, federations, or credible exits to another polity.

### Identity lifecycle

Workstation render modes distinguish process and signing core, but key
rotation, compromise, recovery, snapshot forks, migration, retirement, and
destruction remain mostly display concepts rather than causal events.

### Dispute and repair

Objections influence appraisal but rarely become conversations. Communities
cannot clarify, answer, withdraw, reaffirm, repair harm, make restitution, or
preserve durable disagreement under explicit process.

## Research priority

1. Pending signature and reviewer attention.
2. Two communities with different signed policies.
3. Bundle/body exchange across a costly courier link.
4. Explicit resource authority versus host enforcement.
5. Custody, manifest disclosure, and archive drift.
6. Identity lifecycle and contested succession.
7. Virgils and measurable inherited framing.

This order produces new insight at each increment and avoids building an
archipelago whose settlements still share one invisible law.

## Architectural recommendation

Retain the classic-script, shared-state model for the first experiments:

```text
world.js    topology and operation costs
sim.js      event/state kernel and local epistemic views
render.js   projections, never hidden authority
ui.js       controls and inspection
```

Introduce explicit modules conceptually before splitting files mechanically:

```text
proposal lifecycle
policy appraisal
transport/custody
authority/enforcement
identity lifecycle
```

Use actual Rust-generated fixtures or WASM only where byte-level verification
is being tested. Social-policy experiments should remain fast and deterministic.

## Guiding questions

- What must a participant know before a signature is worth giving?
- When does clarification reduce harm, and when does it create paralysis?
- Can valid-but-opaque proposals defeat a community through attention cost?
- Can incompatible local laws exchange evidence without importing authority?
- How does a community detect divergence between legitimate grants and host
  action when it cannot prevent the host from acting?
- What does a custodian reveal merely by describing the shape of an archive?
- Which differences between participant judgments arise from evidence, policy,
  interpreter, or inherited reading path?

