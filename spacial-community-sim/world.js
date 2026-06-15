// Comms Village — world.js
//
// Terrain, layout, and the COST FIELD. This file owns the answer to "did we
// really create distance?": every spatial relationship in the simulation is
// expressed as a cost between two locations, computed from the terrain the
// map actually shows. Roads are cheap, meadow is normal, woods and standing
// crops are dear. Nothing in here consumes simulation entropy or DOM —
// costs are pure functions of the terrain, so the sim stays deterministic
// and the harness stays headless.
//
// Public surface:
//   buildWorld(p)                          -> world (assigned by sim.js)
//   rebuildCostField()                     -> call after any terrain edit
//   cost(operation, a, b, params)          -> general cost query
//   travelCost(a, b, params)               -> convenience: cost("travel", ...)
//   messagingCost(a, b, params)            -> convenience: cost("messaging", ...)
//   registerCostOp(name, op)               -> extension point for new operations
//   claimHome(pool) / releaseHome(v)       -> plot lifecycle (grows on demand)

"use strict";

const hasDom = typeof document !== "undefined" && typeof window !== "undefined";

// --- Generic math ------------------------------------------------------------

function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)); }
function lerp(a, b, t) { return a + (b - a) * t; }
function dist(a, b) { return Math.hypot(a.x - b.x, a.y - b.y); }

// Layout PRNG — separate from the simulation PRNG so terrain doesn't consume
// simulation entropy. The instance is kept on `world` so plots generated
// mid-run (housing growth) stay deterministic given the same claim order.
function makeRng(seed) {
  let s = seed;
  return () => {
    s = (s * 1664525 + 1013904223) % 4294967296;
    return s / 4294967296;
  };
}

let world; // assigned in sim.js seedState(); declared here, first script

// --- World layout ----------------------------------------------------------------
// Normalized 1x1 map. The village proper sits center-left; an outlying
// farmstead cluster sits up the north-east road. Note what is now ABSENT:
// the farmstead has no special simulation flag. Its lag is emergent — it is
// simply far away, and the cost field makes far expensive.

function buildWorld(p) {
  return p.worldMode === "workstation" ? buildWorkstationWorld(p) : buildVillageWorld(p);
}

function buildVillageWorld(p) {
  const rng = makeRng(20260611);
  const center = { x: 0.40, y: 0.56 };
  const commons = { x: 0.305, y: 0.50, r: 0.052 };
  const market = { x: 0.485, y: 0.565, r: 0.058 };
  const farmstead = { x: 0.815, y: 0.205, r: 0.07 };
  const camp = { x: 0.265, y: 0.715 };          // where newcomers wait

  const homes = [];
  const count = 40;                              // founding plots; growth adds more
  for (let i = 0; i < count; i += 1) {
    const angle = (Math.PI * 2 * i) / count + rng() * 0.12;
    const radius = 0.135 + rng() * 0.10;
    const x = center.x + Math.cos(angle) * radius * 1.25;
    const y = center.y + Math.sin(angle) * radius;
    if (dist({ x, y }, commons) < commons.r + 0.04) continue;
    if (dist({ x, y }, market) < market.r + 0.04) continue;
    homes.push({ x, y, claimed: false });
  }

  const farmHomes = [];
  for (let i = 0; i < 8; i += 1) {
    const a = rng() * Math.PI * 2;
    farmHomes.push({
      x: farmstead.x + Math.cos(a) * (0.02 + rng() * 0.045),
      y: farmstead.y + Math.sin(a) * (0.015 + rng() * 0.04),
      claimed: false
    });
  }

  const trees = [];
  for (let i = 0; i < 64; i += 1) {
    const x = rng();
    const y = rng();
    const pt = { x, y };
    if (dist(pt, center) < 0.27 || dist(pt, farmstead) < 0.12) continue;
    if (dist(pt, camp) < 0.06) continue;
    trees.push({ x, y, r: 0.006 + rng() * 0.008 });
  }

  const fields = [
    { x: 0.60, y: 0.80, w: 0.14, h: 0.085, angle: -0.12 },
    { x: 0.13, y: 0.30, w: 0.12, h: 0.075, angle: 0.18 },
    { x: 0.74, y: 0.33, w: 0.115, h: 0.07, angle: 0.32 },
    { x: 0.90, y: 0.12, w: 0.10, h: 0.06, angle: 0.05 }
  ];

  // Roads: village -> farmstead, the south road newcomers arrive by, and the
  // market-commons lane. Roads are now causal: the cost ops discount them.
  const roads = [
    [{ x: 0.46, y: 0.50 }, { x: 0.58, y: 0.40 }, { x: 0.70, y: 0.30 }, { x: farmstead.x, y: farmstead.y }],
    [{ x: 0.36, y: 0.62 }, { x: 0.30, y: 0.73 }, { x: 0.22, y: 0.86 }, { x: 0.14, y: 0.99 }],
    [{ x: 0.46, y: 0.58 }, { x: 0.40, y: 0.55 }, { x: 0.355, y: 0.52 }]
  ];

  const stalls = [];
  for (let i = 0; i < goodsTable.length; i += 1) {
    const a = (Math.PI * 2 * i) / goodsTable.length - 0.5;
    stalls.push({
      x: market.x + Math.cos(a) * market.r * 0.72,
      y: market.y + Math.sin(a) * market.r * 0.72,
      good: goodsTable[i]
    });
  }

  const w = {
    kind: "village", center, commons, market, farmstead, camp,
    homes, farmHomes, trees, fields, roads, stalls, rng
  };
  buildCostFieldFor(w);
  return w;
}

function buildWorkstationWorld(p) {
  const rng = makeRng(20260615);
  const center = { x: 0.49, y: 0.53 };
  const commons = { x: 0.50, y: 0.16, r: 0.065 };
  const market = { x: 0.50, y: 0.53, r: 0.09 };
  const farmstead = { x: 0.88, y: 0.47, r: 0.08 };
  const camp = { x: 0.12, y: 0.84 };
  const resources = [
    { id: "language", label: "language model", x: 0.38, y: 0.46, vram: 12, color: "#6c8fd3" },
    { id: "vision", label: "vision model", x: 0.62, y: 0.46, vram: 10, color: "#b96fc7" },
    { id: "speech", label: "speech I/O", x: 0.39, y: 0.62, vram: 5, color: "#55a9a1" },
    { id: "embedding", label: "embedding model", x: 0.61, y: 0.62, vram: 4, color: "#83a85d" },
    { id: "cpu", label: "CPU workers", x: 0.25, y: 0.54, vram: 0, color: "#c28b4c" },
    { id: "storage", label: "shared store", x: 0.75, y: 0.54, vram: 0, color: "#5d8ba1" },
    { id: "remote", label: "remote gateway", x: 0.91, y: 0.10, vram: 0, color: "#c38bb0" }
  ];
  const homes = [];
  const farmHomes = [];
  const vmCells = [];
  const columns = 6;
  for (let i = 0; i < 36; i += 1) {
    const side = i % 2;
    const row = Math.floor(i / 2) % columns;
    const bank = Math.floor(i / 12);
    const x = side ? 0.91 - bank * 0.065 : 0.09 + bank * 0.065;
    const y = 0.145 + row * 0.125;
    const plot = {
      x, y, claimed: false,
      vm: `${vmNames[i % vmNames.length]}-${String(Math.floor(i / vmNames.length) + 1).padStart(2, "0")}`
    };
    homes.push(plot);
    vmCells.push({ ...plot, w: 0.07, h: 0.085, bank });
  }
  for (let i = 0; i < 8; i += 1) {
    const plot = {
      x: i % 2 ? 0.90 : 0.82,
      y: 0.27 + Math.floor(i / 2) * 0.17,
      claimed: false,
      vm: `waypoint-${String(i + 1).padStart(2, "0")}`
    };
    farmHomes.push(plot);
    vmCells.push({ ...plot, w: 0.06, h: 0.07, bank: 3 });
  }
  const roads = [
    [{ x: 0.15, y: 0.08 }, { x: 0.15, y: 0.90 }],
    [{ x: 0.85, y: 0.08 }, { x: 0.85, y: 0.90 }],
    [{ x: 0.15, y: 0.53 }, { x: 0.85, y: 0.53 }],
    [{ x: 0.50, y: 0.16 }, { x: 0.50, y: 0.78 }],
    [{ x: 0.50, y: 0.53 }, { x: 0.72, y: 0.34 }, { x: 0.91, y: 0.10 }],
    [{ x: 0.12, y: 0.84 }, { x: 0.28, y: 0.72 }, { x: 0.50, y: 0.53 }]
  ];
  const stalls = resources.map((r) => ({ x: r.x, y: r.y, good: { id: r.id, color: r.color } }));
  const assemblages = [
    { x: 0.055, y: 0.085, w: 0.19, h: 0.78, label: "local agent bank" },
    { x: 0.775, y: 0.20, w: 0.17, h: 0.67, label: "assemblage subnet" }
  ];
  const remoteZone = {
    x: 0.825, y: 0.035, w: 0.15, h: 0.12,
    label: "external service boundary"
  };
  const keyManagers = {
    host: [{ id: "host-km", label: "host key manager", x: 0.50, y: 0.77 }],
    federated: [
      { id: "local-km", label: "local-bank key manager", x: 0.29, y: 0.78 },
      { id: "assemblage-km", label: "assemblage key manager", x: 0.71, y: 0.78 }
    ]
  };
  const ledger = { label: "signature-only host ledger", x: 0.50, y: 0.91 };
  const w = {
    kind: "workstation", center, commons, market, farmstead, camp,
    homes, farmHomes, trees: [], fields: [], roads, stalls, resources,
    vmCells, assemblages, remoteZone, keyManagers, ledger, vramCapacity: 24, rng
  };
  buildCostFieldFor(w);
  return w;
}

// --- Cost field --------------------------------------------------------------------
// A coarse grid over the map. Each cell records which terrain features cover
// it; cost OPERATIONS turn those features into a movement multiplier. The
// grid is rebuilt only when terrain changes (reset, or a future terrain
// edit), never per frame and never per query.

const COST_GRID_N = 96;

function pointSegDist(p, a, b) {
  const abx = b.x - a.x;
  const aby = b.y - a.y;
  const len2 = abx * abx + aby * aby;
  const t = len2 > 0 ? clamp(((p.x - a.x) * abx + (p.y - a.y) * aby) / len2, 0, 1) : 0;
  return { d: Math.hypot(p.x - (a.x + abx * t), p.y - (a.y + aby * t)), t };
}

function inField(p, f) {
  const dx = p.x - f.x;
  const dy = p.y - f.y;
  const cos = Math.cos(-f.angle);
  const sin = Math.sin(-f.angle);
  const lx = dx * cos - dy * sin;
  const ly = dx * sin + dy * cos;
  return Math.abs(lx) <= f.w / 2 && Math.abs(ly) <= f.h / 2;
}

function buildCostFieldFor(w) {
  const n = COST_GRID_N;
  const ROAD_HALF_WIDTH = 0.012;
  const cells = new Array(n * n);
  for (let gy = 0; gy < n; gy += 1) {
    for (let gx = 0; gx < n; gx += 1) {
      const p = { x: (gx + 0.5) / n, y: (gy + 0.5) / n };
      let road = false;
      for (const r of w.roads) {
        for (let i = 0; i < r.length - 1 && !road; i += 1) {
          if (pointSegDist(p, r[i], r[i + 1]).d < ROAD_HALF_WIDTH) road = true;
        }
        if (road) break;
      }
      let tree = 0;
      for (const t of w.trees) {
        const reach = t.r * 2.4;       // a tree's thicket extends past its crown
        const d = dist(p, t);
        if (d < reach) tree += (1 - d / reach) * 0.8;
      }
      let field = false;
      for (const f of w.fields) {
        if (inField(p, f)) { field = true; break; }
      }
      cells[gy * n + gx] = { road, tree: clamp(tree, 0, 1), field };
    }
  }
  w.costField = { n, cells };

  // Road geometry with cumulative arc lengths, for road-aware routing.
  w.roadGeom = w.roads.map((pts) => {
    const cum = [0];
    for (let i = 1; i < pts.length; i += 1) cum.push(cum[i - 1] + dist(pts[i - 1], pts[i]));
    return { pts, cum };
  });
}

// Call after any terrain mutation (the right-click editor will lean on this).
function rebuildCostField() {
  buildCostFieldFor(world);
}

// --- Bus-aware link routing ----------------------------------------------------------
// The visual twin of the cost field's "walk to a road, ride it, walk off": a
// relationship line gets onto the nearest bus, runs along it, and steps off to
// the other end, instead of teleporting across the cell. Pure geometry over
// `roadGeom` (normalized coords, RNG-free), so determinism survives.

// Nearest projection of p onto one road polyline; returns the foot point, its
// arc-length s along the polyline, and the distance d off the road.
function projectOntoRoad(p, geom) {
  let best = { d: Infinity, s: 0, point: geom.pts[0] };
  for (let i = 0; i < geom.pts.length - 1; i += 1) {
    const a = geom.pts[i];
    const b = geom.pts[i + 1];
    const { d, t } = pointSegDist(p, a, b);
    if (d < best.d) {
      best = {
        d,
        s: geom.cum[i] + t * (geom.cum[i + 1] - geom.cum[i]),
        point: { x: lerp(a.x, b.x, t), y: lerp(a.y, b.y, t) }
      };
    }
  }
  return best;
}

// Fan parallel links apart along the bus: shift each run point perpendicular to
// the local bus direction by lane * gap. Endpoints (the real nodes) are left be.
function laneOffsetPts(pts, lane) {
  if (!lane) return pts;
  const gap = 0.006 * lane;
  return pts.map((p, i) => {
    const prev = pts[Math.max(0, i - 1)];
    const next = pts[Math.min(pts.length - 1, i + 1)];
    const dx = next.x - prev.x;
    const dy = next.y - prev.y;
    const len = Math.max(1e-6, Math.hypot(dx, dy));
    return { x: p.x - (dy / len) * gap, y: p.y + (dx / len) * gap };
  });
}

function dedupePath(pts) {
  const out = [];
  for (const p of pts) {
    const last = out[out.length - 1];
    if (!last || Math.hypot(p.x - last.x, p.y - last.y) > 1e-4) out.push(p);
  }
  return out.length >= 2 ? out : pts.slice(0, 2);
}

// Route a → b along the single bus that costs the least off-road stub, the way
// the cost model picks the cheapest road. Returns a normalized polyline
// [a, entry, ...interior bus vertices..., exit, b]; falls back to [a, b].
function busRoute(a, b, lane = 0) {
  const geoms = world && world.roadGeom;
  if (!geoms || !geoms.length) return [a, b];
  let pick = null;
  for (const geom of geoms) {
    const ea = projectOntoRoad(a, geom);
    const eb = projectOntoRoad(b, geom);
    const stub = ea.d + eb.d;
    if (!pick || stub < pick.stub) pick = { stub, geom, ea, eb };
  }
  const { geom, ea, eb } = pick;
  const lo = Math.min(ea.s, eb.s);
  const hi = Math.max(ea.s, eb.s);
  const mids = [];
  for (let i = 0; i < geom.pts.length; i += 1) {
    if (geom.cum[i] > lo + 1e-6 && geom.cum[i] < hi - 1e-6) mids.push(geom.pts[i]);
  }
  if (ea.s > eb.s) mids.reverse();
  const run = laneOffsetPts([ea.point, ...mids, eb.point], lane);
  return dedupePath([a, ...run, b]);
}

// Point at fractional arc length t in [0,1] along a polyline (any units), so a
// directional marker can ride a routed path the way it rides a Bézier.
function pointAlongPath(pts, t) {
  if (!pts || pts.length === 0) return { x: 0, y: 0 };
  if (pts.length === 1) return { x: pts[0].x, y: pts[0].y };
  const seg = [];
  let total = 0;
  for (let i = 1; i < pts.length; i += 1) {
    const d = dist(pts[i - 1], pts[i]);
    seg.push(d);
    total += d;
  }
  if (total <= 0) return { x: pts[0].x, y: pts[0].y };
  let target = clamp(t, 0, 1) * total;
  for (let i = 0; i < seg.length; i += 1) {
    if (target <= seg[i] || i === seg.length - 1) {
      const f = seg[i] > 0 ? target / seg[i] : 0;
      return { x: lerp(pts[i].x, pts[i + 1].x, f), y: lerp(pts[i].y, pts[i + 1].y, f) };
    }
    target -= seg[i];
  }
  const last = pts[pts.length - 1];
  return { x: last.x, y: last.y };
}

function costCellAt(x, y) {
  const n = world.costField.n;
  const gx = clamp(Math.floor(x * n), 0, n - 1);
  const gy = clamp(Math.floor(y * n), 0, n - 1);
  return world.costField.cells[gy * n + gx];
}

// --- Cost operations -----------------------------------------------------------------
// An operation answers two questions: what multiplier does a terrain cell
// impose (`cell`), and what multiplier does riding a road impose (`road`)?
// Params flow through so the same operation can answer differently for a
// villager with a cart, a courier on horseback, a sealed bundle, etc.

const costOps = Object.create(null);

function registerCostOp(name, op) {
  costOps[name] = op;
}

// Walking. Carts ride roads cheaply but suffer off-road and in the woods.
registerCostOp("travel", {
  road(params) { return params && params.cart ? 0.45 : 0.62; },
  cell(cell, params) {
    const cart = Boolean(params && params.cart);
    let m = 1;
    if (cell.field) m = cart ? 1.7 : 1.35;     // standing crops: you go around
    m += cell.tree * (cart ? 1.5 : 0.9);        // woods slow everyone, carts more
    if (cell.road) m = Math.min(m, this.road(params));
    return m;
  }
});

// Word of mouth / a carried message. Tracks travel for now but discounts
// roads harder (news moves along them with every passer-by) and is dampened
// more by woods between hearths. This is the hook the courier tier will use.
registerCostOp("messaging", {
  road() { return 0.5; },
  cell(cell) {
    let m = 1;
    if (cell.field) m = 1.2;
    m += cell.tree * 1.1;
    if (cell.road) m = Math.min(m, this.road());
    return m;
  }
});

// Terrain integral along the straight segment a->b under an operation.
function lineCost(a, b, op, params) {
  const len = dist(a, b);
  if (len < 1e-6) return 0;
  const steps = Math.max(2, Math.min(160, Math.ceil(len * COST_GRID_N * 1.2)));
  const stepLen = len / steps;
  let total = 0;
  for (let i = 0; i < steps; i += 1) {
    const t = (i + 0.5) / steps;
    const cell = costCellAt(lerp(a.x, b.x, t), lerp(a.y, b.y, t));
    total += op.cell(cell, params) * stepLen;
  }
  return total;
}

function nearestOnPolyline(geom, p) {
  let best = { d: Infinity, point: geom.pts[0], arc: 0 };
  for (let i = 0; i < geom.pts.length - 1; i += 1) {
    const a = geom.pts[i];
    const b = geom.pts[i + 1];
    const hit = pointSegDist(p, a, b);
    if (hit.d < best.d) {
      best = {
        d: hit.d,
        point: { x: lerp(a.x, b.x, hit.t), y: lerp(a.y, b.y, hit.t) },
        arc: geom.cum[i] + dist(a, b) * hit.t
      };
    }
  }
  return best;
}

// The general query. People are not laser beams: the cost of getting from a
// to b is the cheaper of going straight (terrain integral) or walking to a
// road, riding it, and walking off — evaluated per road. No pathfinding;
// a cost model, not a physics engine, and exactly as deterministic.
function cost(operation, a, b, params = {}) {
  const op = costOps[operation];
  if (!op) throw new Error(`unknown cost operation: ${operation}`);
  let best = lineCost(a, b, op, params);
  const roadMult = op.road(params);
  for (const geom of world.roadGeom) {
    const onA = nearestOnPolyline(geom, a);
    const onB = nearestOnPolyline(geom, b);
    const via = lineCost(a, onA.point, op, params)
      + Math.abs(onA.arc - onB.arc) * roadMult
      + lineCost(onB.point, b, op, params);
    if (via < best) best = via;
  }
  return best;
}

function travelCost(a, b, params) { return cost("travel", a, b, params); }
function messagingCost(a, b, params) { return cost("messaging", a, b, params); }

// --- Plot lifecycle ---------------------------------------------------------------
// Housing now grows instead of collapsing onto plot zero. When a pool is
// exhausted, a new plot is surveyed: candidate sites ring the settlement and
// the cheapest commute wins — so growth creeps along the roads, the way real
// villages do. Villagers hold a reference to their plot (not a copy), which
// is what the coming house inspector will key on.

function claimHome(pool) {
  const free = pool.filter((h) => !h.claimed);
  let home = free.length ? free[Math.floor(rand() * free.length)] : null;
  if (!home) {
    home = surveyNewPlot(pool);
    pool.push(home);
  }
  home.claimed = true;
  return home;
}

function releaseHome(v) {
  if (v.home && typeof v.home.claimed === "boolean") v.home.claimed = false;
}

function surveyNewPlot(pool) {
  if (world.kind === "workstation") return surveyNewVm(pool);
  const farm = pool === world.farmHomes;
  const anchor = farm ? world.farmstead : world.center;
  const claimedCount = pool.filter((h) => h.claimed).length;
  const ringBase = (farm ? 0.075 : 0.16) + claimedCount * 0.0014; // settlement creeps outward
  let best = null;
  let bestScore = Infinity;
  for (let k = 0; k < 16; k += 1) {
    const a = world.rng() * Math.PI * 2;
    const radius = ringBase + world.rng() * 0.09;
    const cand = {
      x: clamp(anchor.x + Math.cos(a) * radius * 1.25, 0.02, 0.98),
      y: clamp(anchor.y + Math.sin(a) * radius, 0.02, 0.98)
    };
    if (dist(cand, world.commons) < world.commons.r + 0.035) continue;
    if (dist(cand, world.market) < world.market.r + 0.035) continue;
    if (tooCrowded(cand)) continue;
    const score = travelCost(cand, world.market) + travelCost(cand, world.commons) * 0.5;
    if (score < bestScore) {
      bestScore = score;
      best = cand;
    }
  }
  if (!best) {
    best = {
      x: clamp(anchor.x + (world.rng() - 0.5) * 0.4, 0.02, 0.98),
      y: clamp(anchor.y + (world.rng() - 0.5) * 0.4, 0.02, 0.98)
    };
  }
  return { x: best.x, y: best.y, claimed: false };
}

function surveyNewVm(pool) {
  const edge = pool === world.farmHomes;
  const claimed = pool.filter((h) => h.claimed).length;
  const row = claimed % 7;
  const layer = Math.floor(claimed / 7);
  const plot = edge
    ? {
        x: clamp(0.88 - layer * 0.025, 0.72, 0.94),
        y: clamp(0.12 + row * 0.11, 0.08, 0.92)
      }
    : {
        x: claimed % 2 ? 0.91 - layer * 0.025 : 0.09 + layer * 0.025,
        y: clamp(0.12 + row * 0.12, 0.08, 0.92)
      };
  plot.vm = `provision-${String(world.vmCells.length + 1).padStart(2, "0")}`;
  world.vmCells.push({ ...plot, w: 0.06, h: 0.075, bank: 4 + layer });
  return { ...plot, claimed: false };
}

function tooCrowded(cand) {
  for (const pools of [world.homes, world.farmHomes]) {
    for (const h of pools) {
      if (h.claimed && dist(cand, h) < 0.022) return true;
    }
  }
  return false;
}
  const vmNames = [
    "cinder", "lattice", "keystone", "parallax", "quorum", "meridian",
    "aperture", "vector", "relay", "harbor", "prism", "archive"
  ];
