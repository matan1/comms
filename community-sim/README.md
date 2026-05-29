# Comms Community Simulator

Static browser demo for exploring community deal trust, ceremony commitment rules,
attestation frequency, sponsor requirements, objections, and a seed-floor
resource economy with a small goods market.

Open `index.html` in a browser, or serve the directory with:

```sh
python3 -m http.server 8080 -d community-sim
```

The app is intentionally separate from the Python tool suite. It models the
same concepts from `docs/comms.spec.1.0.md` at simulation level: ceremony
records, endorsements, objections, recognitions, allocation returns, and an
auditable event graph.

The goods market treats allocation grants as purchasing credits. Stewards
produce one abstract good, consume a basket of goods, and buy what they need
from the current supply. Exchange rates can be fixed by rule or floating. In
floating mode, prices move from supply/demand pressure plus trusted
`price-signal/1` attestations emitted by market participants. Cycles also emit
sampled `purchase-decision/1` records and a `market-clearing/1` decision so the
price path remains auditable.

Deal trust is derived from behavior: completed agreements raise it and failed
negotiations lower it. The initial prior slider only seeds new stewards before
their trade and ceremony history takes over.

## Auto-Tune

The Auto-Tune panel searches for balanced simulator parameters without any
models, training, or network services. It runs candidate parameter sets
off-screen — the visible simulation is never mutated until you apply a result —
and ranks them against a "Healthy Balance" target: high mean deal trust, good
market satisfaction, low unmet demand, moderate inequality, and continued
ceremony/admission activity.

Pick a search budget (Quick / Normal / Deep) and press **Run tune**. The tuner
uses a deterministic randomized/grid hybrid: it always includes the current
settings plus balanced anchors across the commit-rule and exchange-rate options,
then fills the remaining budget with randomized samples over conservative ranges.
Each candidate runs for a fixed horizon and is scored on the average of its
recent cycles. Population and run speed are held fixed so candidates stay
comparable to the current scenario.

Ranked candidates show the score, deal trust, satisfaction, unmet demand, Gini,
steward count, and an admission summary, with a short explanation of each
candidate's strongest and weakest dimension and which controls it changes. Press
**Apply** on a candidate to copy its settings into the live controls and reset
the simulation.
