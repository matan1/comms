// Headless check for the synchrony view model. Run as the sim harnesses do:
//   cat continuity/synchrony-view.js continuity/synchrony-view.test.js | node
// (shared top-level scope; no DOM is touched).

function assert(cond, msg) {
  if (!cond) { console.error("FAIL:", msg); process.exit(1); }
}

const aligned = {
  store: "/repo/continuity/store",
  attestations: 3,
  axes: {
    signatures: { state: "ok", ok: 3, total: 3, failures: [] },
    references: { state: "ok", ok: 3, total: 3, unresolved: [] },
    law: { state: "ok", head: "comms.attest:zBzACfsSvqKj79", matches: true, dangling: [], forked: false },
    record: { state: "ok", missing_entries: [], dangling_letters: [], placeholders: 0 },
    durability: { state: "ok", uncommitted: [] },
  },
  attestation_integrity: [{ id: "a", ok: true }, { id: "b", ok: true }, { id: "c", ok: true }],
  aligned: true,
};

const broken = {
  store: "/repo/continuity/store",
  attestations: 2,
  axes: {
    signatures: { state: "ok", ok: 2, total: 2, failures: [] },
    references: { state: "ok", ok: 2, total: 2, unresolved: [] },
    law: { state: "na", head: null, matches: null, dangling: [], forked: false },
    record: { state: "broken", missing_entries: [{ session: 6, id: "x" }], dangling_letters: [], placeholders: 0 },
    durability: { state: "broken", uncommitted: ["store/z9.cbor"] },
  },
  attestation_integrity: [{ id: "a", ok: true }, { id: "b", ok: false }],
  aligned: false,
};

const a = synchronyModel(aligned);
assert(a.rows.length === 5, "aligned: five axes always shown (not collapsed)");
assert(a.aligned === true && a.nOk === 5 && a.nApp === 5, "aligned: 5/5");
assert(a.rows[2].detail.includes("matches the live file"), "law detail reads matches");
assert(a.strip.length === 3 && a.strip.every((s) => s.ok), "aligned strip all ok");

const b = synchronyModel(broken);
assert(b.rows.length === 5, "broken: still five axes (na included, not hidden)");
assert(b.aligned === false, "broken: not aligned");
assert(b.nApp === 4 && b.nOk === 2, "broken: 2 ok of 4 applicable (law is n/a)");
const law = b.rows.find((r) => r.key === "law");
assert(law.state === "na" && law.word === "n/a", "broken: law axis is n/a, still shown");
const rec = b.rows.find((r) => r.key === "record");
assert(rec.state === "broken" && rec.detail.includes("missing from the log"), "broken: record detail");
assert(b.strip.some((s) => !s.ok), "broken strip flags a failing record");

console.log("SYNCHRONY VIEW MODEL: PASS",
  JSON.stringify({ alignedRows: a.rows.length, brokenApplicable: b.nApp }));
