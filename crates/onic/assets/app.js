const canvas = document.getElementById("graph");
const statusEl = document.getElementById("status");
const form = document.getElementById("filters");
const kindsBox = document.getElementById("kinds");
const detailTitle = document.getElementById("detail-title");
const detailModeBtn = document.getElementById("detail-mode");
const detailEmpty = document.getElementById("detail-empty");
const detailStyled = document.getElementById("detail-styled");
const detailRaw = document.getElementById("node-json");
const dKind = document.getElementById("d-kind");
const dName = document.getElementById("d-name");
const dFile = document.getElementById("d-file");
const dLines = document.getElementById("d-lines");
const dBody = document.getElementById("d-body");
const dEdges = document.getElementById("d-edges");

const KIND_OPTIONS = ["file", "symbol", "doc", "heading", "comment", "tag", "unresolved"];
const COLORS = {
  file: "#005ea1",
  symbol: "#2b78bf",
  doc: "#7b5500",
  heading: "#9a6c00",
  comment: "#5f5e60",
  tag: "#474649",
  unresolved: "#ba1a1a",
};
const MIN_SCALE = 0.25;
const MAX_SCALE = 4;
const PAN_SLOP = 4;

for (const kind of KIND_OPTIONS) {
  const label = document.createElement("label");
  label.innerHTML = `<input type="checkbox" name="kind" value="${kind}" checked /> ${kind}`;
  kindsBox.append(label);
}

let state = {
  nodes: [],
  edges: [],
  pos: new Map(),
  selected: null,
  camera: { x: 0, y: 0, scale: 1 },
  detailMode: "styled",
  payload: null,
};

let drag = null;

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  await loadGraph();
});

detailModeBtn.addEventListener("click", () => {
  state.detailMode = state.detailMode === "styled" ? "raw" : "styled";
  renderDetail();
});

canvas.addEventListener(
  "wheel",
  (event) => {
    event.preventDefault();
    const point = canvasPoint(event);
    const oldScale = state.camera.scale;
    const next = clamp(oldScale * Math.exp(-event.deltaY * 0.001), MIN_SCALE, MAX_SCALE);
    const k = next / oldScale;
    state.camera.x = point.x - (point.x - state.camera.x) * k;
    state.camera.y = point.y - (point.y - state.camera.y) * k;
    state.camera.scale = next;
    draw();
  },
  { passive: false },
);

canvas.addEventListener("pointerdown", (event) => {
  canvas.setPointerCapture(event.pointerId);
  const point = canvasPoint(event);
  const world = screenToWorld(point.x, point.y);
  drag = {
    startX: point.x,
    startY: point.y,
    camX: state.camera.x,
    camY: state.camera.y,
    hit: hitNode(world.x, world.y),
    moved: false,
  };
});

canvas.addEventListener("pointermove", (event) => {
  if (!drag) {
    return;
  }
  const point = canvasPoint(event);
  const dx = point.x - drag.startX;
  const dy = point.y - drag.startY;
  if (!drag.moved && Math.hypot(dx, dy) < PAN_SLOP) {
    return;
  }
  drag.moved = true;
  canvas.classList.add("panning");
  state.camera.x = drag.camX + dx;
  state.camera.y = drag.camY + dy;
  draw();
});

canvas.addEventListener("pointerup", async (event) => {
  if (!drag) {
    return;
  }
  const finished = drag;
  drag = null;
  canvas.classList.remove("panning");
  if (canvas.hasPointerCapture(event.pointerId)) {
    canvas.releasePointerCapture(event.pointerId);
  }
  if (finished.moved || !finished.hit) {
    return;
  }
  await selectNode(finished.hit.id);
});

canvas.addEventListener("pointercancel", (event) => {
  drag = null;
  canvas.classList.remove("panning");
  if (canvas.hasPointerCapture(event.pointerId)) {
    canvas.releasePointerCapture(event.pointerId);
  }
});

async function loadGraph() {
  const q = document.getElementById("q").value.trim();
  const seed = document.getElementById("seed").value.trim();
  const hops = document.getElementById("hops").value;
  const limit = document.getElementById("limit").value;
  const kinds = [...document.querySelectorAll("input[name=kind]:checked")].map((el) => el.value);
  const params = new URLSearchParams({ hops, limit, kinds: kinds.join(",") });
  if (q) params.set("q", q);
  if (seed) params.set("seed", seed);
  const res = await fetch(`/api/subgraph?${params}`);
  const data = await res.json();
  if (data.error) {
    statusEl.textContent = data.error;
    return;
  }
  state.nodes = data.nodes ?? [];
  state.edges = data.edges ?? [];
  state.selected = null;
  state.payload = null;
  resetCamera();
  layout();
  draw();
  renderDetail();
  statusEl.textContent = `${state.nodes.length} nodes, ${state.edges.length} edges (cap ${data.cap}, hops ${data.hops}). Scroll to zoom. Drag to pan.`;
}

function layout() {
  const pos = new Map();
  const n = state.nodes.length || 1;
  state.nodes.forEach((node, i) => {
    const angle = (i / n) * Math.PI * 2;
    pos.set(node.id, {
      x: canvas.width / 2 + Math.cos(angle) * 280,
      y: canvas.height / 2 + Math.sin(angle) * 220,
    });
  });
  for (let step = 0; step < 80; step++) {
    for (const edge of state.edges) {
      const a = pos.get(edge.src);
      const b = pos.get(edge.dst);
      if (!a || !b) continue;
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const dist = Math.hypot(dx, dy) || 1;
      const pull = (dist - 120) * 0.05;
      a.x += (dx / dist) * pull;
      a.y += (dy / dist) * pull;
      b.x -= (dx / dist) * pull;
      b.y -= (dy / dist) * pull;
    }
    for (let i = 0; i < state.nodes.length; i++) {
      for (let j = i + 1; j < state.nodes.length; j++) {
        const a = pos.get(state.nodes[i].id);
        const b = pos.get(state.nodes[j].id);
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const dist = Math.hypot(dx, dy) || 1;
        if (dist > 90) continue;
        const push = ((90 - dist) / dist) * 2;
        a.x -= dx * push;
        a.y -= dy * push;
        b.x += dx * push;
        b.y += dy * push;
      }
    }
  }
  state.pos = pos;
}

function draw() {
  const ctx = canvas.getContext("2d");
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.setTransform(state.camera.scale, 0, 0, state.camera.scale, state.camera.x, state.camera.y);
  ctx.strokeStyle = "#c1c7d2";
  ctx.lineWidth = 1 / state.camera.scale;
  for (const edge of state.edges) {
    const a = state.pos.get(edge.src);
    const b = state.pos.get(edge.dst);
    if (!a || !b) continue;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
  }
  for (const node of state.nodes) {
    const p = state.pos.get(node.id);
    if (!p) continue;
    ctx.beginPath();
    ctx.fillStyle = COLORS[node.kind] || "#005ea1";
    ctx.arc(p.x, p.y, node.id === state.selected ? 9 : 6, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "#1a1b1f";
    ctx.font = `${12 / state.camera.scale}px Inter, sans-serif`;
    ctx.fillText(node.name, p.x + 10, p.y + 4);
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
}

function hitNode(x, y) {
  const radius = Math.max(9, 12 / state.camera.scale);
  for (const node of state.nodes) {
    const p = state.pos.get(node.id);
    if (!p) continue;
    if (Math.hypot(p.x - x, p.y - y) < radius) return node;
  }
  return null;
}

function canvasPoint(event) {
  const rect = canvas.getBoundingClientRect();
  return {
    x: ((event.clientX - rect.left) / rect.width) * canvas.width,
    y: ((event.clientY - rect.top) / rect.height) * canvas.height,
  };
}

function screenToWorld(x, y) {
  return {
    x: (x - state.camera.x) / state.camera.scale,
    y: (y - state.camera.y) / state.camera.scale,
  };
}

function resetCamera() {
  state.camera = { x: 0, y: 0, scale: 1 };
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

async function selectNode(id) {
  state.selected = id;
  draw();
  const res = await fetch(`/api/node?id=${encodeURIComponent(id)}`);
  const data = await res.json();
  if (data.error) {
    state.payload = null;
    detailEmpty.textContent = data.error;
    detailEmpty.hidden = false;
    detailStyled.hidden = true;
    detailRaw.hidden = true;
    detailModeBtn.hidden = true;
    return;
  }
  state.payload = data;
  renderDetail();
}

function renderDetail() {
  const payload = state.payload;
  if (!payload) {
    detailTitle.textContent = "Node";
    detailEmpty.hidden = false;
    detailEmpty.textContent = "Select a node.";
    detailStyled.hidden = true;
    detailRaw.hidden = true;
    detailModeBtn.hidden = true;
    return;
  }
  const node = payload.node;
  detailTitle.textContent = node.name || "Node";
  detailEmpty.hidden = true;
  detailModeBtn.hidden = false;
  detailModeBtn.textContent = state.detailMode === "styled" ? "Raw" : "Styled";
  const showRaw = state.detailMode === "raw";
  detailStyled.hidden = showRaw;
  detailRaw.hidden = !showRaw;
  detailRaw.textContent = JSON.stringify(payload, null, 2);
  dKind.textContent = node.kind || "-";
  dName.textContent = node.name || "-";
  dFile.textContent = node.filePath || "-";
  dLines.textContent =
    node.startLine != null && node.endLine != null ? `${node.startLine}-${node.endLine}` : "-";
  dBody.textContent = node.body || "No body.";
  dEdges.replaceChildren();
  for (const neighbor of payload.neighbors ?? []) {
    const item = document.createElement("li");
    item.tabIndex = 0;
    const dir = document.createElement("span");
    dir.className = "edge-dir";
    dir.textContent = neighbor.direction;
    const kind = document.createElement("span");
    kind.className = "edge-kind";
    kind.textContent = neighbor.edgeKind;
    const name = document.createElement("span");
    name.className = "edge-name";
    name.textContent = neighbor.name;
    item.append(dir, kind, name);
    item.addEventListener("click", () => {
      void selectNode(neighbor.id);
    });
    dEdges.append(item);
  }
}
