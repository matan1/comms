// Comms Village — render.js
//
// Everything that paints. The renderer is a pure consumer of `state` and
// `world`: it owns the canvas, the transient pulse effects, and the walking
// animation, and it computes nothing the simulation depends on. Since the
// presence rework, villagers walk to the spot their interactions were
// computed from — the animation is a replay of consequences, not theater.
//
// Headless note: this file declares `pulses` (sim pushes into it) but no
// function here runs without a DOM, so a Node harness can concatenate it
// safely.

"use strict";

let pulses = [];   // transient visual events (attestation births, gossip)

const interactionColors = {
  deal: "#b87815",
  help: "#2f7d66",
  ceremony: "#2e6f9e",
  gossip: "#4f8f8b"
};

const adversaryColors = {
  classic: "#8f3f71",
  sleeper: "#5968a6",
  selective: "#b45742",
  parasite: "#85752f",
  charmer: "#c45f91",
  ghost: "#71818c",
  freeRider: "#b0823d",
  cultivator: "#4f8a68",
  factionist: "#7e4ca1",
  infiltrator: "#405d86",
  ideologue: "#a44e55",
  brinksman: "#c46d2d",
  flash: "#d13f3f",
  patriarch: "#684a3e",
  wrecker: "#8b3131",
  sovereign: "#4f3f91"
};

function ripple(a, b) {
  if (pulses.length > 140) return;
  pulses.push({
    kind: "gossip",
    x: ((a.spot || a.pos).x + (b.spot || b.pos).x) / 2,
    y: ((a.spot || a.pos).y + (b.spot || b.pos).y) / 2,
    t: 0,
    color: "#4f8f8b"
  });
}

function scaledContext(canvas) {
  const ratio = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(canvas.clientWidth * ratio));
  const height = Math.max(1, Math.round(canvas.clientHeight * ratio));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  const ctx = canvas.getContext("2d");
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  return ctx;
}

function colorForTrust(t) {
  if (t > 0.72) return "#2f7d66";
  if (t > 0.48) return "#6a8f5a";
  if (t > 0.3) return "#b87815";
  return "#b65345";
}

function colorForVouch(outcome) {
  if (outcome === "trusted") return "#2f7d66";
  if (outcome === "rejected") return "#b65345";
  if (outcome === "contested") return "#8a5a9c";
  return "#9aa39b";
}

function hexToRgba(hex, a) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a.toFixed(3)})`;
}

function drawHouse(ctx, x, y) {
  ctx.fillStyle = "#cbb592";
  ctx.fillRect(x - 6, y - 4, 12, 9);
  ctx.strokeStyle = "rgba(96, 80, 52, 0.55)";
  ctx.lineWidth = 1;
  ctx.strokeRect(x - 6, y - 4, 12, 9);
  ctx.beginPath();
  ctx.moveTo(x - 7.5, y - 4);
  ctx.lineTo(x, y - 11);
  ctx.lineTo(x + 7.5, y - 4);
  ctx.closePath();
  ctx.fillStyle = "#a05f43";
  ctx.fill();
}

function drawWorld(ctx, w, h) {
  // Meadow
  const grad = ctx.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, "#c3d2ab");
  grad.addColorStop(1, "#aec39a");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, w, h);

  // Fields
  for (const f of world.fields) {
    ctx.save();
    ctx.translate(f.x * w, f.y * h);
    ctx.rotate(f.angle);
    const fw = f.w * w;
    const fh = f.h * h;
    ctx.fillStyle = "#cdbd86";
    ctx.fillRect(-fw / 2, -fh / 2, fw, fh);
    ctx.strokeStyle = "rgba(122, 104, 56, 0.35)";
    ctx.lineWidth = 1;
    for (let i = 1; i < 6; i += 1) {
      const y = -fh / 2 + (fh * i) / 6;
      ctx.beginPath();
      ctx.moveTo(-fw / 2 + 3, y);
      ctx.lineTo(fw / 2 - 3, y);
      ctx.stroke();
    }
    ctx.strokeStyle = "rgba(90, 78, 44, 0.5)";
    ctx.strokeRect(-fw / 2, -fh / 2, fw, fh);
    ctx.restore();
  }

  // Roads
  for (const road of world.roads) {
    ctx.beginPath();
    ctx.moveTo(road[0].x * w, road[0].y * h);
    for (const pt of road.slice(1)) ctx.lineTo(pt.x * w, pt.y * h);
    ctx.strokeStyle = "#d9c9a4";
    ctx.lineWidth = 9;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.stroke();
    ctx.strokeStyle = "rgba(124, 106, 70, 0.4)";
    ctx.lineWidth = 1;
    ctx.setLineDash([5, 7]);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Commons: a stone circle, the ceremony ground.
  const c = world.commons;
  ctx.beginPath();
  ctx.arc(c.x * w, c.y * h, c.r * w, 0, Math.PI * 2);
  ctx.fillStyle = "#d6d2bb";
  ctx.fill();
  ctx.strokeStyle = "rgba(96, 92, 70, 0.6)";
  ctx.lineWidth = 1.5;
  ctx.stroke();
  for (let i = 0; i < 9; i += 1) {
    const a = (Math.PI * 2 * i) / 9;
    ctx.beginPath();
    ctx.arc(c.x * w + Math.cos(a) * c.r * w * 0.82, c.y * h + Math.sin(a) * c.r * w * 0.82, 2.6, 0, Math.PI * 2);
    ctx.fillStyle = "#8b8674";
    ctx.fill();
  }

  // Market: stalls with goods-colored awnings.
  const m = world.market;
  ctx.beginPath();
  ctx.arc(m.x * w, m.y * h, m.r * w, 0, Math.PI * 2);
  ctx.fillStyle = "#dccfae";
  ctx.fill();
  ctx.strokeStyle = "rgba(124, 106, 70, 0.45)";
  ctx.stroke();
  for (const stall of world.stalls) {
    const sx = stall.x * w;
    const sy = stall.y * h;
    ctx.fillStyle = "#8a6a45";
    ctx.fillRect(sx - 6, sy - 3, 12, 7);
    ctx.fillStyle = stall.good.color;
    ctx.beginPath();
    ctx.moveTo(sx - 8, sy - 3);
    ctx.lineTo(sx + 8, sy - 3);
    ctx.lineTo(sx + 6, sy - 9);
    ctx.lineTo(sx - 6, sy - 9);
    ctx.closePath();
    ctx.fill();
  }

  // Newcomers' camp
  const camp = world.camp;
  ctx.beginPath();
  ctx.moveTo(camp.x * w - 8, camp.y * h + 6);
  ctx.lineTo(camp.x * w, camp.y * h - 8);
  ctx.lineTo(camp.x * w + 8, camp.y * h + 6);
  ctx.closePath();
  ctx.fillStyle = "#c8b48e";
  ctx.fill();
  ctx.strokeStyle = "rgba(96, 80, 52, 0.6)";
  ctx.stroke();

  // Homes (village ring + farmstead + growth), claimed plots only.
  for (const pool of [world.homes, world.farmHomes]) {
    for (const homePlot of pool) {
      if (!homePlot.claimed) continue;
      drawHouse(ctx, homePlot.x * w, homePlot.y * h);
    }
  }

  // Trees
  for (const t of world.trees) {
    ctx.beginPath();
    ctx.arc(t.x * w, t.y * h, t.r * w, 0, Math.PI * 2);
    ctx.fillStyle = "#7fa06b";
    ctx.fill();
    ctx.beginPath();
    ctx.arc(t.x * w - t.r * w * 0.3, t.y * h - t.r * w * 0.35, t.r * w * 0.62, 0, Math.PI * 2);
    ctx.fillStyle = "#92b27c";
    ctx.fill();
  }

  // Labels
  ctx.fillStyle = "rgba(45, 50, 40, 0.72)";
  ctx.font = "italic 12px Georgia, 'Iowan Old Style', serif";
  ctx.fillText("the commons", c.x * w - 32, (c.y - c.r) * h - 8);
  ctx.fillText("market square", m.x * w - 36, (m.y + m.r) * h + 18);
  ctx.fillText("the farmstead", world.farmstead.x * w - 36, (world.farmstead.y - 0.085) * h);
  ctx.fillText("newcomers' camp", camp.x * w - 44, camp.y * h + 24);
}

function drawVillagers(ctx, w, h) {
  const vp = viewpoint();
  for (const v of state.villagers) {
    const x = v.pos.x * w;
    const y = v.pos.y * h;
    const r = 5 + v.capability * 4;

    let fill;
    let faded = false;
    if (vp) {
      if (v.id === vp.id) {
        fill = colorForTrust(state.cached.mean.get(v.id) ?? state.prior);
      } else if (params().vouchMode) {
        const judgment = perceivedVouch(vp, v.id);
        fill = colorForVouch(judgment.outcome);
        faded = judgment.outcome === "awaiting-context";
      } else if (isStrangerTo(vp, v.id)) {
        fill = "#9aa39b";
        faded = true;
      } else {
        fill = colorForTrust(perceivedTrust(vp, v.id));
      }
    } else {
      fill = v.adversaryType
        ? adversaryColors[v.adversaryType]
        : (v.member ? colorForTrust(state.cached.mean.get(v.id) ?? state.prior) : "#b87815");
    }

    ctx.globalAlpha = faded ? 0.45 : 1;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fillStyle = fill;
    ctx.fill();
    ctx.lineWidth = v.member ? 1.4 : 1;
    ctx.strokeStyle = v.member ? "rgba(30, 37, 39, 0.55)" : "rgba(30, 37, 39, 0.3)";
    if (!v.member) ctx.setLineDash([2, 2]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;

    // Ground truth is a debug privilege: defectors are only marked when no
    // villager's viewpoint is selected.
    if (!vp && (v.archetype === "defector" || v.archetype === "adversary")) {
      ctx.save();
      ctx.setLineDash([3, 3]);
      ctx.beginPath();
      ctx.arc(x, y, r + 3, 0, Math.PI * 2);
      ctx.strokeStyle = "rgba(94, 53, 110, 0.9)";
      ctx.lineWidth = 1.5;
      ctx.stroke();
      ctx.restore();
    }

    if (vp && v.id === vp.id) {
      ctx.beginPath();
      ctx.arc(x, y, r + 4, 0, Math.PI * 2);
      ctx.strokeStyle = "#1e2527";
      ctx.lineWidth = 2;
      ctx.stroke();
      ctx.fillStyle = "#1e2527";
      ctx.font = "bold 12px ui-monospace, monospace";
      ctx.fillText(v.label, x + r + 6, y + 4);
    } else if (!vp && v.adversaryType) {
      ctx.fillStyle = "rgba(30, 37, 39, 0.88)";
      ctx.font = "bold 10px ui-monospace, monospace";
      ctx.fillText(`${v.label} · ${v.adversaryType}`, x + r + 4, y + 4);
    } else if (!vp && (v.capability > 0.95 || !v.member)) {
      ctx.fillStyle = "rgba(30, 37, 39, 0.75)";
      ctx.font = "11px ui-monospace, monospace";
      ctx.fillText(v.label, x + r + 4, y + 4);
    }
  }
}

function curvedLinkGeometry(w, h, edge) {
  const a = state.byId.get(edge.from);
  const b = state.byId.get(edge.to);
  if (!a || !b) return null;
  const x1 = a.pos.x * w;
  const y1 = a.pos.y * h;
  const x2 = b.pos.x * w;
  const y2 = b.pos.y * h;
  const dx = x2 - x1;
  const dy = y2 - y1;
  const len = Math.max(1, Math.hypot(dx, dy));
  const bend = (edge.lane || 0) * Math.min(7, len * 0.045);
  const cx = (x1 + x2) / 2 - (dy / len) * bend;
  const cy = (y1 + y2) / 2 + (dx / len) * bend;
  return { x1, y1, x2, y2, cx, cy };
}

function drawCurvedLink(ctx, w, h, edge, color, width, alpha, progress, dashed = false) {
  const path = curvedLinkGeometry(w, h, edge);
  if (!path) return;
  ctx.save();
  if (dashed) ctx.setLineDash([4, 5]);
  ctx.beginPath();
  ctx.moveTo(path.x1, path.y1);
  ctx.quadraticCurveTo(path.cx, path.cy, path.x2, path.y2);
  ctx.strokeStyle = hexToRgba(color, alpha);
  ctx.lineWidth = width;
  ctx.stroke();

  if (edge.directional !== false) {
    const t = progress;
    const mt = 1 - t;
    const x = mt * mt * path.x1 + 2 * mt * t * path.cx + t * t * path.x2;
    const y = mt * mt * path.y1 + 2 * mt * t * path.cy + t * t * path.y2;
    ctx.setLineDash([]);
    ctx.beginPath();
    ctx.arc(x, y, Math.max(1.8, width * 1.15), 0, Math.PI * 2);
    ctx.fillStyle = hexToRgba(color, Math.min(1, alpha + 0.25));
    ctx.fill();
  }
  ctx.restore();
}

function drawInteractions(ctx, w, h, revealProgress = null) {
  if (revealProgress === null) return;
  const overlay = state.interactionOverlay || { direct: [], evidence: [] };
  const envelope = Math.sin(Math.PI * revealProgress);
  const markerProgress = Math.min(1, revealProgress * 1.35);
  for (const edge of overlay.evidence) {
    const colors = {
      positive: "#2f7d66",
      negative: "#b65345",
      endorsement: "#6a8f5a",
      ceremony: "#2e6f9e",
      context: "#637074"
    };
    drawCurvedLink(
      ctx, w, h, edge, colors[edge.class] || colors.context,
      edge.counted ? 1.35 : 0.8,
      (edge.counted ? 0.36 : 0.18) * envelope,
      markerProgress,
      !edge.counted
    );
  }
  for (const edge of overlay.direct) {
    const failed = ["failed", "not-admitted"].includes(edge.outcome);
    drawCurvedLink(
      ctx, w, h, edge,
      failed ? "#b65345" : (interactionColors[edge.kind] || "#637074"),
      2.4, 0.72 * envelope, markerProgress
    );
  }
}

function drawPulses(ctx, w, h, dt) {
  for (const pulse of pulses) {
    pulse.t += dt;
    const life = pulse.kind === "gossip" ? 1.4 : 1.8;
    const k = pulse.t / life;
    if (k >= 1) continue;
    const alpha = (1 - k) * (pulse.kind === "gossip" ? 0.5 : 0.85);
    ctx.beginPath();
    ctx.arc(pulse.x * w, pulse.y * h, 3 + k * (pulse.kind === "gossip" ? 22 : 14), 0, Math.PI * 2);
    ctx.strokeStyle = hexToRgba(pulse.color, alpha);
    ctx.lineWidth = pulse.kind === "gossip" ? 1.2 : 2;
    ctx.stroke();
  }
  pulses = pulses.filter((pulse) => pulse.t < 2);
}

function prepareJourneys(seconds) {
  const timing = phaseTiming(seconds);
  for (const v of state.villagers) {
    const distance = dist(v.pos, v.target);
    v.journey = distance < 0.00005 ? null : {
      from: { ...v.pos },
      to: { ...v.target },
      elapsed: 0,
      duration: timing.travel
    };
  }
}

function moveVillagers(dt) {
  for (const v of state.villagers) {
    if (!v.journey) continue;
    v.journey.elapsed = Math.min(v.journey.duration, v.journey.elapsed + dt);
    const t = v.journey.elapsed / v.journey.duration;
    const eased = t * t * (3 - 2 * t);
    v.pos.x = v.journey.from.x + (v.journey.to.x - v.journey.from.x) * eased;
    v.pos.y = v.journey.from.y + (v.journey.to.y - v.journey.from.y) * eased;
    if (t >= 1) {
      v.pos = { ...v.journey.to };
      v.journey = null;
    }
  }
}
