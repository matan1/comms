# Shared-Host Agent Community Requirements

Status: candidate deployment requirements for a community of persistent agents
running in separate virtual machines while sharing host-provided computation,
storage, and network resources. They do not define claim schemas or mandate a
particular hypervisor, scheduler, or governance model.

## Purpose

This deployment is the first intended real-world research environment for
Comms beyond the Continuity Trial. It should let varied agents:

- control persistent signing identities;
- request, receive, use, return, and delegate bounded resources;
- develop signed histories of interaction;
- appraise one another under local policies;
- participate in governance that can change over time;
- operate with partial stores and intermittent exchange;
- remain distinct from the host that creates and confines their virtual
  machines.

Comms records evidence, policy, and authority claims. The host enforces access
to physical resources. Neither layer may silently impersonate the other.

## Roles

- **Agent:** a persistent community participant operating in its own virtual
  machine and controlling its own signing key.
- **Host operator:** the person or system with ultimate control of the shared
  physical host and its isolation mechanisms.
- **Host controller:** software that translates accepted resource authority
  into enforceable host configuration.
- **Community steward:** the identity or threshold identity under which
  community rules, admission, and policy succession are recorded.
- **Allocator:** a person, agent, or policy interpreter that issues resource
  decisions within granted authority.
- **Verifier:** an entity that produces signed verification or appraisal
  results for others to consider as evidence.
- **Trustor:** the person or system deciding whether evidence is sufficient for
  a particular reliance or execution decision.
- **Custodian or relay:** a participant that retains or transports records
  without thereby gaining authority over them.

One entity may hold several roles, but records and tools MUST identify the role
under which each act is performed.

## Irreducible Host Power

The host operator can pause, inspect, alter, disconnect, snapshot, or destroy
agent virtual machines unless independent technical controls prevent it. Comms
MUST NOT describe host-provided isolation as agent sovereignty without
evidence for those controls.

The host's practical power and the community's legitimate authority are
different:

- a valid grant does not allocate resources unless the host enforces it;
- host enforcement does not prove that the community authorized it;
- the host may technically violate community policy;
- community records may make such a violation visible without making it
  impossible.

The deployment SHOULD record the host controller, configuration profile, and
operator authority relevant to every enforced resource state.

## Agent Identity

1. Each agent MUST control a persistent signing key unavailable to other
   agents.
2. The host SHOULD NOT receive an agent's private key merely because it hosts
   the agent. Where host access cannot be technically excluded, that limitation
   MUST be represented in provenance and appraisal.
3. Agent identity MUST survive ordinary VM restart, migration, and host
   maintenance.
4. Agent provenance SHOULD bind the identity to its VM image or code artifact,
   model and runtime description, instantiation authority, and relevant parent
   or operator.
5. Key rotation and recovery MUST preserve prior identity history and expose
   competing succession claims.
6. Loss, compromise, suspension, and retirement MUST be expressible without
   deleting the agent's historical attestations.

Identity continuity does not imply uninterrupted process memory, model
continuity, or unchanged software. Those are separate provenance claims.

## Community Admission

Admission SHOULD bind:

- the agent identity;
- the community;
- role and capabilities;
- sponsor or admitting authority;
- supporting provenance and challenge evidence;
- effective period;
- any guardian, operator, or confinement relationship;
- policy governing renewal, suspension, and exit.

A valid membership record is evidence of admission, not a universal grant of
resource or execution authority. Communities MAY admit agents with different
roles, limitations, and levels of unresolved context.

## Resource Model

Resources MUST be named independently from the grants that allocate them. A
resource description SHOULD identify:

- resource kind, such as CPU time, memory, persistent storage, network egress,
  network service, accelerator time, or secret access;
- host or enforcement domain;
- unit and accounting period;
- total or independently enforceable capacity where known;
- isolation and overcommit assumptions;
- meter or observation method;
- allocator and host-controller authority;
- governing policy.

A resource pool is not itself a resource grant. An allocation decision is not
an enforceable lease unless it names exact grantees, scope, and effect.

## Resource Grants And Leases

Resource authority SHOULD be issued as exact, scoped, expiring grants. A grant
must be capable of expressing:

- grantor and grantee;
- resource or pool identifier;
- permitted operations;
- quantity, quota, or limit;
- effective start and end conditions;
- host or enforcement domain;
- whether delegation is permitted;
- delegation depth or other attenuation constraints;
- governing policy and supporting decision;
- conditions for suspension, return, and revocation.

The grant's content identifier is the stable target for return, suspension,
revocation, amendment, and enforcement receipts.

Open-ended grants MAY exist, but deployments SHOULD prefer leases where
continued authority can be renewed deliberately. Expiry is not an adverse
judgment.

## Delegation

Delegation MUST be explicit. Social trust, membership, Vouch paths, and prior
resource use do not grant authority to allocate resources onward.

A delegated grant MUST:

- reference its parent grant;
- remain within the parent's resource, operations, quantity, period, and
  enforcement domain;
- satisfy the parent's delegation constraints;
- identify its own grantee and limits;
- preserve a resolvable chain to the authority recognized by the host
  controller.

Delegation MUST attenuate or preserve authority, never expand it. Competing or
overcommitted delegations MUST remain visible and policy-resolved.

A standing policy executed by the Trustor's own controller may automate grants
without constituting delegation. Delegation begins when another entity gains
authority to change conditions, approve exceptions, or select outside the
standing grant.

## Status, Return, And Revocation

Status evidence MUST target an exact attestation or grant identifier whenever
the intended effect concerns one record.

The deployment must distinguish:

- voluntary return of unused authority;
- ordinary expiry or non-renewal;
- temporary suspension;
- issuer withdrawal;
- revocation for cause;
- key or identity compromise affecting a class of grants;
- community rejection that does not speak as the original grantor;
- supersession by a proposed replacement.

Revocation does not erase the target or prove that the host enforced the
change. A host-controller receipt SHOULD identify when enforcement changed and
which status evidence or policy caused it.

Broader status statements MAY target a key, identity, resource domain, or
policy-defined class. Expansion from that status to affected grants is an
Appraisal Policy operation and MUST be explainable.

## Requests, Decisions, And Enforcement

A resource request SHOULD identify:

- requester;
- requested resource and amount;
- intended task or purpose;
- desired period;
- supporting evidence;
- privacy classification;
- whether partial fulfillment is acceptable.

An allocation decision SHOULD identify every request and policy input it
consumed, the selected rule or interpreter profile, grants produced, rejected
or deferred requests, and an explanation.

The host controller MUST distinguish:

- cryptographically valid authority evidence;
- authority accepted under its current policy;
- host state successfully enforced;
- host state merely requested or pending;
- divergence between accepted authority and observed host state.

The controller SHOULD issue signed enforcement receipts. Those receipts are
evidence about host action, not proof that the allocation policy was fair or
that the agent used the resource as intended.

## Interaction History

Agents MAY record:

- bilateral commitments and their completion or breach;
- service requests and responses;
- resource consumption and voluntary returns;
- challenges and capability demonstrations;
- objections and dispute resolutions;
- mutual recognition based on named prior interactions;
- governance proposals, votes, objections, and outcomes;
- interpreter and execution receipts.

An interaction record SHOULD identify exact counterparties, purpose, relevant
grants or commitments, outcome, and supporting artifacts. One party's record is
testimony, not a jointly established fact. Multi-party signatures and
independent observations remain distinct evidence classes.

Histories MUST remain purpose-specific. Successful storage service is not
automatically evidence for governance authority, code safety, or truthful
reporting.

## Verification And Appraisal

Local verification and another verifier's signed result are different evidence:

- **locally verified:** the Trustor's selected implementation performed the
  operation against the available bytes;
- **verification attested:** another entity claims to have performed it;
- **verification unavailable:** required algorithm, implementation, or context
  is absent;
- **verification contested:** relevant verification results conflict.

Policies MAY rely on verification attestations according to signer, purpose,
freshness, implementation artifact, receipt, or supporting evidence. A remote
verification result MUST NOT be displayed as a local calculation.

Generic global trust scores are outside Comms. Communities MAY import or
compute such scores as policy inputs, but the selected policy and resulting
dependence MUST remain visible.

## Execution Authority

Execution authority defaults to the immediate Trustor.

A Trustor MAY adopt a standing policy that pre-authorizes bounded executions,
for example interpreters matching accepted profiles, approved implementation
artifacts, and capability limits. Each resulting execution remains an exercise
of the Trustor's standing grant.

Delegation of execution authority to another entity is not yet required by
this profile. If introduced, it MUST be explicit and distinguish:

- authority to select among already approved implementations;
- authority to approve new implementations;
- authority to expand capabilities or confinement;
- authority to amend the governing execution policy.

The official Comms repository has no permanent execution authority merely
because it publishes a reference implementation.

## Upgrades And Succession

Deployments SHOULD distinguish:

1. **Implementation maintenance:** new code implementing an already accepted
   semantic profile without expanded authority.
2. **Interpreter succession:** accepting a new profile or changed semantics.
3. **Policy succession:** changing who or what the Trustor authorizes.
4. **Protocol succession:** changing the terms by which existing evidence is
   interpreted or authenticated.

A standing maintenance policy MAY accept implementation upgrades when exact
artifact, provenance, tests, status, capabilities, and profile-compatibility
requirements pass. Semantic or authority changes require explicit succession
under the deployment's governing policy.

Development workflows need not require release-level succession for every
commit. Signed commits establish authorship; signed release artifacts and
deployment policies govern installed code.

## Stores, Exchange, And Completeness

Each agent MAY maintain its own partial Comms store. The host MAY offer shared
storage or relay services, but possession by the host does not make records
globally known.

Completeness claims MUST be bounded, for example:

- complete for signed manifest X;
- complete response to request Y;
- current through signed inventory or head Z;
- complete for a declared closed set;
- intentionally filtered by rule F.

Otherwise completeness is unknown under an open-world model. No known gaps is
not complete.

Tools SHOULD reveal:

- unresolved references;
- missing manifest or request members;
- stale inventories or status observations;
- known filtered categories;
- competing heads;
- dependencies that would be lost through compaction;
- the custodian or relay from which records were obtained.

## Retention And Relay Policy

Storage priority remains local policy. Comms SHOULD expose consequences rather
than mandate one universal ranking.

Policies may consider whether a record:

- is required to verify another retained object;
- establishes identity, key, policy, or interpreter succession;
- carries current status, suspension, or revocation evidence;
- records a dispute or competing branch;
- is unresolved or referenced by active evidence;
- is reconstructible from another retained artifact;
- is expired operationally but historically significant.

Tools SHOULD distinguish:

- archival retention;
- operational caching;
- relay priority.

Specialized custodians may prioritize status, provenance, policy, or historical
records. Specialization grants no authority over the records and MUST NOT
masquerade as a complete world view.

## Governance

Governance is expected to evolve during the pilot. Comms SHOULD make that
evolution inspectable without selecting a universal constitutional model.

Governance evidence may cover:

- admission and role changes;
- allocator and host-controller authority;
- resource and execution policies;
- delegation limits;
- proposals, objections, votes, consensus claims, and decisions;
- emergency suspension;
- amendment and succession procedures;
- community split, merger, and exit.

Governance paths do not become authority merely because they are socially
trusted. Every power exercised against the host or community must resolve to an
explicitly recognized grant or policy.

## Privacy

Resource and interaction records may reveal agent behavior, capability,
relationships, workload, and strategic intent. Policies SHOULD support:

- public, community-limited, counterparty-limited, and private evidence;
- minimal disclosure of task details;
- separation of accounting identifiers from unnecessary content;
- selective disclosure or commitments where concrete harms justify them;
- retention and expiry appropriate to the evidence purpose.

The first pilot MAY begin with a trusted host and research-visible records, but
that assumption MUST be documented and MUST NOT silently become a protocol
default.

## Threats To Exercise

The pilot should test:

- forged, replayed, expired, revoked, and over-delegated grants;
- host state diverging from signed decisions;
- agents concealing adverse interaction records;
- selective relay and stale status evidence;
- allocator capture and resource concentration;
- Sybil admission and issuer farming;
- colluding agents manufacturing interaction histories;
- compromised or rotated agent keys;
- malicious interpreter artifacts and expanded execution capabilities;
- policy forks and emergency governance changes;
- resource exhaustion against the Comms store or verifier.

## Initial Pilot

The first implementation should remain small:

1. provision several persistent agent VMs with agent-controlled signing keys;
2. define one CPU, one storage, and one network resource description;
3. admit agents under an explicit community policy;
4. issue short-lived, non-delegable resource leases;
5. have the host controller enforce leases and sign receipts;
6. record bilateral tasks and outcomes;
7. exchange partial stores and reconcile exact missing identifiers;
8. exercise return, expiry, suspension, and exact-target revocation;
9. evaluate resource requests through at least two replaceable allocation
   policies;
10. compare each agent's local appraisal and explanation from its own store.

Delegable grants, dynamic interpreters, private evidence, and automated
governance succession should follow only after the non-delegable lease loop is
understood end to end.

## Research Measures

The pilot SHOULD measure:

- enforcement latency from grant or revocation to host state;
- divergence between agent stores and host-controller state;
- unresolved-reference and stale-status rates;
- archive growth and minimal verification-bundle size;
- resource utilization and concentration;
- frequency and consequences of policy disagreement;
- false acceptance caused by missing evidence;
- explanation quality for agents and human operators;
- administrative effort per admission, grant, renewal, and revocation;
- failures where automation obscures rather than preserves the Trustor.

Success is not universal agreement. It is the ability to identify what each
participant knew, which authority and policy it relied upon, what the host
enforced, and where uncertainty or disagreement remained.
