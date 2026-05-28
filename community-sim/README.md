# Comms Community Simulator

Static browser demo for exploring community trust, ceremony commitment rules,
attestation frequency, sponsor requirements, objections, and a seed-floor
resource economy.

Open `index.html` in a browser, or serve the directory with:

```sh
python3 -m http.server 8080 -d community-sim
```

The app is intentionally separate from the Python tool suite. It models the
same concepts from `docs/comms.spec.1.0.md` at simulation level: ceremony
records, endorsements, objections, recognitions, allocation returns, and an
auditable event graph.
