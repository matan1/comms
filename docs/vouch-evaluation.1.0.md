# Vouch 1.0 Candidate Evaluation

Run date: 2026-06-14. Harness:
`spacial-community-sim/vouch-adversary-test.js`.

The comparison used 25 deterministic seeds for each of 16 adversary presets,
120 simulated days per run, and matched honest-newcomer controls. The flat
model and Vouch model received the same scenario parameters. Vouch counted
distinct direct interaction issuers, treated objections as contestation, and
stopped a viewer relying on a steward after two independent failures reached
that viewer's store.

## Acceptance criteria

- At least 25% aggregate reduction in harmful post-admission deals.
- No more than a five-point reduction in honest admission rate.
- No more than two additional median days to honest admission.
- No adversary preset with more than 15% harm regression.

## Results

| Preset | Flat harm | Vouch harm |
|---|---:|---:|
| classic | 714 | 46 |
| sleeper | 610 | 50 |
| selective | 604 | 48 |
| parasite | 197 | 40 |
| charmer | 500 | 42 |
| ghost | 188 | 37 |
| freeRider | 162 | 39 |
| cultivator | 198 | 33 |
| factionist | 279 | 37 |
| infiltrator | 167 | 35 |
| ideologue | 524 | 42 |
| brinksman | 387 | 36 |
| flash | 1186 | 49 |
| patriarch | 193 | 38 |
| wrecker | 844 | 44 |
| sovereign | 335 | 39 |

- Aggregate harm reduction: **90.8%**.
- Honest admission: **80.0% flat → 88.0% Vouch**.
- Honest median admission delay: **0 days**.
- Worst preset harm regression: **-75.9%** (every preset improved).

The result is evidence for the informative reference profile, not proof that
the policy generalizes to real communities. The simulator remains illustrative
and its attestations are not wire-encoded protocol objects.
