// Synchrony view — the browser projection of `continuity_ceremony.py synchrony
// --json`. The terminal renders the same data as ANSI tumblers; this renders it
// for a human, in color, as separate tumblers that must each be read — never
// collapsed into one checkmark (Cartographer: a map shows signs, not answers).
//
// No build, no dependencies. The model functions are pure and DOM-free so a
// Node harness can check them; mountSynchrony touches the DOM only when present.

const AXIS_ORDER = ["signatures", "references", "law", "record", "durability"];

const AXIS_BLURB = {
  signatures: "every record is signed by the key it claims",
  references: "every reference resolves in the store",
  law: "the live constitution is the current, signed head",
  record: "the trial-log reflects the store; declared letters exist",
  durability: "the store is committed, not just in the working tree",
};

const STATE_WORD = { ok: "in sync", broken: "out of sync", na: "n/a" };

function shortId(id) {
  if (!id || typeof id !== "string") return "";
  const tail = id.split(":").pop();
  return tail.length > 10 ? tail.slice(0, 10) + "…" : tail;
}

// One human-readable detail line per axis, mirroring the terminal view's logic.
function axisDetail(key, a) {
  if (key === "signatures") {
    return `${a.ok}/${a.total} records verify`;
  }
  if (key === "references") {
    return a.state === "broken"
      ? `${a.ok}/${a.total} resolve · ${a.unresolved.length} unresolved`
      : `${a.ok}/${a.total} resolve`;
  }
  if (key === "law") {
    if (a.state === "na") return "no constitution in the store";
    if (a.matches && (!a.dangling || a.dangling.length === 0)) {
      return `head ${shortId(a.head)} matches the live file`;
    }
    if (a.forked) return "the rule chain forks";
    if (a.dangling && a.dangling.length) return "a supersedes-target is missing";
    return "the live file differs from the head";
  }
  if (key === "record") {
    if (a.state === "ok") return "trial-log reflects the store; no dangling letters";
    return `${a.missing_entries.length} entr(y/ies) missing from the log, `
      + `${a.dangling_letters.length} dangling letter(s), `
      + `${a.placeholders} placeholder(s)`;
  }
  if (key === "durability") {
    if (a.state === "na") return "no git context";
    return a.state === "ok"
      ? "committed"
      : `${a.uncommitted.length} store file(s) uncommitted`;
  }
  return "";
}

// Pure model the renderer (browser or test) consumes.
function synchronyModel(data) {
  const axes = (data && data.axes) || {};
  const rows = AXIS_ORDER.filter((k) => axes[k]).map((k) => ({
    key: k,
    state: axes[k].state,
    word: STATE_WORD[axes[k].state] || axes[k].state,
    detail: axisDetail(k, axes[k]),
    blurb: AXIS_BLURB[k],
  }));
  const strip = (data && data.attestation_integrity) || [];
  const nApp = rows.filter((r) => r.state !== "na").length;
  const nOk = rows.filter((r) => r.state === "ok").length;
  return {
    store: (data && data.store) || "",
    attestations: (data && data.attestations) || 0,
    rows,
    strip,
    aligned: Boolean(data && data.aligned),
    nOk,
    nApp,
  };
}

// --- DOM rendering (browser only) --------------------------------------------------
function mountSynchrony(root, data) {
  if (typeof document === "undefined" || !root) return;
  const m = synchronyModel(data);
  root.innerHTML = "";

  const head = document.createElement("div");
  head.className = "sync-head";
  head.textContent = `${m.attestations} attestations · ${m.store || "—"}`;
  root.appendChild(head);

  for (const r of m.rows) {
    const row = document.createElement("div");
    row.className = `sync-row state-${r.state}`;
    row.innerHTML =
      `<span class="pip" aria-hidden="true"></span>` +
      `<span class="axis">${r.key}</span>` +
      `<span class="word">${r.word}</span>` +
      `<span class="detail">${r.detail}</span>` +
      `<span class="blurb">${r.blurb}</span>`;
    root.appendChild(row);
  }

  if (m.strip.length) {
    const strip = document.createElement("div");
    strip.className = "sync-strip";
    const label = document.createElement("span");
    label.className = "strip-label";
    label.textContent = "records";
    strip.appendChild(label);
    for (const rec of m.strip) {
      const cell = document.createElement("span");
      cell.className = `cell ${rec.ok ? "ok" : "broken"}`;
      cell.title = (rec.id || "") + (rec.ok ? " · verifies" : " · FAILS");
      strip.appendChild(cell);
    }
    root.appendChild(strip);
  }

  const sum = document.createElement("div");
  sum.className = `sync-summary ${m.aligned ? "aligned" : "partial"}`;
  sum.textContent = m.aligned
    ? `${m.nOk}/${m.nApp} axes aligned · the lock is open`
    : `${m.nOk}/${m.nApp} axes aligned · not yet whole`;
  root.appendChild(sum);
}

// Make the model available to a Node harness (the sim's shared-scope idiom).
if (typeof module !== "undefined" && module.exports) {
  module.exports = { synchronyModel, axisDetail, mountSynchrony, AXIS_ORDER };
}
