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
      fill = v.member ? colorForTrust(state.cached.mean.get(v.id) ?? state.prior) : "#b87815";
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
    } else if (!vp && (v.capability > 0.95 || !v.member)) {
      ctx.fillStyle = "rgba(30, 37, 39, 0.75)";
      ctx.font = "11px ui-monospace, monospace";
      ctx.fillText(v.label, x + r + 4, y + 4);
    }
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

function moveVillagers(dt) {
  const speed = 0.14;
  for (const v of state.villagers) {
    const dx = v.target.x - v.pos.x;
    const dy = v.target.y - v.pos.y;
    const d = Math.hypot(dx, dy);
    const stepLen = Math.min(d, speed * dt);
    if (d > 0.0005) {
      v.pos.x += (dx / d) * stepLen;
      v.pos.y += (dy / d) * stepLen;
    }
    // A faint idle sway so the village never looks frozen.
    const t = performance.now() / 1000 + v.wander;
    v.pos.x += Math.sin(t * 0.9) * 0.00018;
    v.pos.y += Math.cos(t * 0.7) * 0.00015;
  }
}
