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

function ripple(a, b, useHomes = false) {
  if (pulses.length > 140) return;
  const from = useHomes ? a.home : (a.spot || a.pos);
  const to = useHomes ? b.home : (b.spot || b.pos);
  pulses.push({
    kind: "gossip",
    x: (from.x + to.x) / 2,
    y: (from.y + to.y) / 2,
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
  if (world.kind === "workstation") {
    drawWorkstationWorld(ctx, w, h);
    return;
  }
  drawVillageWorld(ctx, w, h);
}

function drawVillageWorld(ctx, w, h) {
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

function roundedRect(ctx, x, y, width, height, radius) {
  const r = Math.min(radius, width / 2, height / 2);
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + width, y, x + width, y + height, r);
  ctx.arcTo(x + width, y + height, x, y + height, r);
  ctx.arcTo(x, y + height, x, y, r);
  ctx.arcTo(x, y, x + width, y, r);
  ctx.closePath();
}

function drawLabelPlate(ctx, text, x, y, options = {}) {
  const font = options.font || "10px ui-monospace, monospace";
  const align = options.align || "center";
  ctx.save();
  ctx.font = font;
  ctx.textAlign = align;
  const metrics = ctx.measureText(text);
  const paddingX = options.paddingX || 5;
  const width = metrics.width + paddingX * 2;
  const height = options.height || 17;
  const left = align === "center" ? x - width / 2 : x - paddingX;
  roundedRect(ctx, left, y - height + 4, width, height, 4);
  ctx.fillStyle = options.background || "rgba(10, 19, 27, 0.82)";
  ctx.fill();
  ctx.fillStyle = options.color || "#e4f0f3";
  ctx.fillText(text, x, y);
  ctx.restore();
}

function drawWorkstationWorld(ctx, w, h) {
  const grad = ctx.createLinearGradient(0, 0, w, h);
  grad.addColorStop(0, "#17222d");
  grad.addColorStop(0.55, "#202b38");
  grad.addColorStop(1, "#111a23");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, w, h);

  const remote = world.remoteZone;
  roundedRect(ctx, remote.x * w, remote.y * h, remote.w * w, remote.h * h, 9);
  ctx.fillStyle = "rgba(95, 48, 80, 0.22)";
  ctx.fill();
  ctx.strokeStyle = "rgba(226, 151, 199, 0.72)";
  ctx.lineWidth = 1.5;
  ctx.setLineDash([5, 5]);
  ctx.stroke();
  ctx.setLineDash([]);
  drawLabelPlate(ctx, remote.label, (remote.x + remote.w / 2) * w,
    (remote.y + 0.025) * h, { color: "#f0cde4" });

  for (const group of world.assemblages) {
    roundedRect(ctx, group.x * w, group.y * h, group.w * w, group.h * h, 10);
    ctx.fillStyle = "rgba(48, 77, 94, 0.12)";
    ctx.fill();
    ctx.strokeStyle = "rgba(111, 171, 192, 0.2)";
    ctx.setLineDash([5, 6]);
    ctx.stroke();
    ctx.setLineDash([]);
    drawLabelPlate(ctx, group.label, group.x * w + 7, group.y * h + 16, {
      align: "left",
      color: "#c4e0e7",
      background: "rgba(18, 32, 43, 0.86)"
    });
  }

  ctx.save();
  ctx.lineCap = "round";
  for (const bus of world.roads) {
    ctx.beginPath();
    ctx.moveTo(bus[0].x * w, bus[0].y * h);
    for (const point of bus.slice(1)) ctx.lineTo(point.x * w, point.y * h);
    ctx.strokeStyle = "rgba(86, 150, 184, 0.18)";
    ctx.lineWidth = 10;
    ctx.stroke();
    ctx.strokeStyle = "rgba(118, 202, 221, 0.65)";
    ctx.lineWidth = 1.2;
    ctx.setLineDash([7, 8]);
    ctx.stroke();
  }
  ctx.restore();

  for (const cell of world.vmCells) {
    const plot = [...world.homes, ...world.farmHomes].find((h) => h.vm === cell.vm);
    const x = (cell.x - cell.w / 2) * w;
    const y = (cell.y - cell.h / 2) * h;
    roundedRect(ctx, x, y, cell.w * w, cell.h * h, 6);
    ctx.fillStyle = "rgba(31, 48, 63, 0.78)";
    ctx.fill();
    ctx.strokeStyle = "rgba(125, 173, 192, 0.32)";
    ctx.lineWidth = 1;
    ctx.stroke();
    if (plot && plot.claimed) {
      ctx.fillStyle = "rgba(199, 229, 236, 0.88)";
      ctx.font = "9px ui-monospace, monospace";
      ctx.textAlign = "left";
      ctx.fillText(cell.vm, x + 5, y + 12);
    }
  }

  const telemetry = state.resourceTelemetry || {};
  for (const node of world.resources) {
    const x = node.x * w;
    const y = node.y * h;
    const radius = node.id === "remote" ? 24 : 20;
    ctx.beginPath();
    ctx.arc(x, y, radius, 0, Math.PI * 2);
    ctx.fillStyle = hexToRgba(node.color, 0.18);
    ctx.fill();
    ctx.strokeStyle = hexToRgba(node.color, 0.85);
    ctx.lineWidth = 1.6;
    ctx.stroke();
    const usage = telemetry.byResource ? telemetry.byResource[node.id] : null;
    if (usage && usage.jobs) {
      const activity = clamp(usage.jobs / 4, 0, 1);
      ctx.beginPath();
      ctx.arc(x, y, radius + 5, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * activity);
      ctx.strokeStyle = usage.failed ? "#e06767" : "#77d2b2";
      ctx.lineWidth = 3;
      ctx.stroke();
    }
    drawLabelPlate(ctx, node.label, x, y + radius + 17, {
      color: "#edf7f9"
    });
    if (node.vram) {
      drawLabelPlate(ctx, `${node.vram} GB · ${usage ? usage.jobs : 0} jobs`,
        x, y + radius + 35, {
          font: "9px ui-monospace, monospace",
          color: "#b9d4dc",
          background: "rgba(10, 19, 27, 0.72)"
        });
    } else if (usage) {
      drawLabelPlate(ctx, `${usage.jobs} jobs`, x, y + radius + 35, {
        font: "9px ui-monospace, monospace",
        color: "#b9d4dc",
        background: "rgba(10, 19, 27, 0.72)"
      });
    }
  }

  const control = world.commons;
  roundedRect(ctx, (control.x - 0.075) * w, (control.y - 0.045) * h, 0.15 * w, 0.09 * h, 8);
  ctx.fillStyle = "rgba(47, 89, 112, 0.52)";
  ctx.fill();
  ctx.strokeStyle = "rgba(123, 205, 223, 0.82)";
  ctx.stroke();
  drawLabelPlate(ctx, "admission controller", control.x * w, control.y * h + 4, {
    color: "#e3f5f8",
    background: "rgba(23, 53, 68, 0.86)"
  });

  roundedRect(ctx, (world.camp.x - 0.055) * w, (world.camp.y - 0.04) * h, 0.11 * w, 0.08 * h, 7);
  ctx.fillStyle = "rgba(154, 111, 74, 0.22)";
  ctx.fill();
  ctx.strokeStyle = "rgba(215, 163, 103, 0.75)";
  ctx.setLineDash([4, 4]);
  ctx.stroke();
  ctx.setLineDash([]);
  drawLabelPlate(ctx, "staging network", world.camp.x * w,
    (world.camp.y - 0.055) * h, {
      color: "#f1d4a9",
      background: "rgba(70, 48, 31, 0.9)"
    });

  drawLabelPlate(ctx, "24 GB shared accelerator envelope",
    world.market.x * w, world.market.y * h - 94, {
      font: "italic 11px Georgia, serif",
      color: "#d4e6eb"
    });
  drawLabelPlate(ctx, "host boundary", 14, 21, {
    align: "left",
    font: "italic 11px Georgia, serif",
    color: "#c1dce4"
  });
}

function drawWorkstationKey(ctx, w, h) {
  const x = w * 0.56;
  const y = h * 0.80;
  const width = Math.min(250, w * 0.18);
  const height = 112;
  roundedRect(ctx, x, y, width, height, 8);
  ctx.fillStyle = "rgba(10, 19, 27, 0.88)";
  ctx.fill();
  ctx.strokeStyle = "rgba(139, 195, 209, 0.48)";
  ctx.stroke();
  ctx.fillStyle = "#e4f0f3";
  ctx.font = "bold 10px ui-monospace, monospace";
  ctx.textAlign = "left";
  ctx.fillText("MAP KEY", x + 12, y + 17);

  const rows = [
    ["vm", "VM boundary"],
    ["core", "persistent signing core"],
    ["agent", "active agent process"],
    ["tether", "process-to-identity tether"],
    ["service", "shared host service"],
    ["bus", "host interconnect"]
  ];
  rows.forEach(([kind, label], i) => {
    const rowY = y + 34 + i * 13;
    ctx.beginPath();
    if (kind === "vm") {
      ctx.strokeStyle = "rgba(155, 205, 217, 0.75)";
      ctx.strokeRect(x + 12, rowY - 7, 14, 9);
    } else if (kind === "core") {
      ctx.arc(x + 19, rowY - 2, 2.5, 0, Math.PI * 2);
      ctx.fillStyle = "#bce1e8";
      ctx.fill();
    } else if (kind === "agent") {
      ctx.arc(x + 19, rowY - 2, 5, 0, Math.PI * 2);
      ctx.fillStyle = "#6a8f5a";
      ctx.fill();
    } else if (kind === "tether") {
      ctx.moveTo(x + 12, rowY - 2);
      ctx.lineTo(x + 26, rowY - 2);
      ctx.strokeStyle = "rgba(119, 190, 209, 0.65)";
      ctx.stroke();
    } else if (kind === "service") {
      ctx.arc(x + 19, rowY - 2, 6, 0, Math.PI * 2);
      ctx.strokeStyle = "#8ab8d0";
      ctx.stroke();
    } else {
      ctx.moveTo(x + 12, rowY - 2);
      ctx.lineTo(x + 26, rowY - 2);
      ctx.setLineDash([3, 3]);
      ctx.strokeStyle = "#76cadd";
      ctx.stroke();
      ctx.setLineDash([]);
    }
    ctx.fillStyle = "#c9dde3";
    ctx.font = "9px ui-monospace, monospace";
    ctx.fillText(label, x + 34, rowY + 1);
  });
}

function drawVillagers(ctx, w, h) {
  const vp = viewpoint();
  const workstation = world.kind === "workstation";
  const labelInk = workstation ? "rgba(225, 240, 244, 0.92)" : "rgba(30, 37, 39, 0.88)";
  for (const v of state.villagers) {
    const x = v.pos.x * w;
    const y = v.pos.y * h;
    const r = 5 + v.capability * 4;

    if (world.kind === "workstation") {
      const hx = v.home.x * w;
      const hy = v.home.y * h;
      ctx.beginPath();
      ctx.moveTo(hx, hy);
      ctx.quadraticCurveTo((hx + x) / 2, (hy + y) / 2 - 10, x, y);
      ctx.strokeStyle = "rgba(119, 190, 209, 0.24)";
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(hx, hy, 2.4, 0, Math.PI * 2);
      ctx.fillStyle = "#bce1e8";
      ctx.fill();
    }

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
    ctx.strokeStyle = workstation
      ? (v.member ? "rgba(220, 239, 244, 0.72)" : "rgba(220, 239, 244, 0.38)")
      : (v.member ? "rgba(30, 37, 39, 0.55)" : "rgba(30, 37, 39, 0.3)");
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
      ctx.strokeStyle = workstation ? "#dceff3" : "#1e2527";
      ctx.lineWidth = 2;
      ctx.stroke();
      ctx.fillStyle = labelInk;
      ctx.font = "bold 12px ui-monospace, monospace";
      ctx.fillText(v.label, x + r + 6, y + 4);
    } else if (!vp && v.adversaryType) {
      ctx.fillStyle = labelInk;
      ctx.font = "bold 10px ui-monospace, monospace";
      ctx.fillText(`${v.label} · ${adversaryName(v.adversaryType)}`, x + r + 4, y + 4);
    } else if (!vp && (v.capability > 0.95 || !v.member)) {
      ctx.fillStyle = workstation ? "rgba(218, 235, 240, 0.8)" : "rgba(30, 37, 39, 0.75)";
      ctx.font = "11px ui-monospace, monospace";
      ctx.fillText(v.label, x + r + 4, y + 4);
    }
  }
}

function curvedLinkGeometry(w, h, edge) {
  const a = state.byId.get(edge.from);
  const b = state.byId.get(edge.to);
  if (!a || !b) return null;
  const useCore = world.kind === "workstation" && interactionEndpointMode() === "core";
  const start = useCore ? a.home : a.pos;
  const end = useCore ? b.home : b.pos;
  const x1 = start.x * w;
  const y1 = start.y * h;
  const x2 = end.x * w;
  const y2 = end.y * h;
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
