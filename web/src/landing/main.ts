// Landing-page behavior (item 10): the node-assembly hero animation, scroll
// reveals, the accent progress rail, and small chrome. No React; this entry
// stays a few KB so the marketing page paints instantly.
//
// The node stage reuses the app's node visual language as a lightweight SVG
// mock (NOT a live @xyflow graph): 112x32 rounded bodies in the category
// pastel fills, typed handle dots, and bezier edges colored by the source
// type, drawing themselves in dependency order as if someone were building a
// subflow. Colors are copied from web/src/styles/tokens.css (category
// pastels, light set) and web/src/registry/datatypes.ts (DATA_TYPE_COLOR).

const CATEGORY_FILLS = ["#dcebfb", "#d9f3e4", "#eae0f7", "#f7e6d7", "#fff7de"];
const WIRE_COLORS = ["#5aa0ff", "#5aa0ff", "#5aa0ff", "#7fd962", "#e879c8"];
const NODE_W = 112;
const NODE_H = 32;

const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Deterministic PRNG (mulberry32), so a given seed builds the same graph. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

interface StageNode {
  x: number;
  y: number;
  col: number;
  fill: string;
  el: SVGGElement;
}

const svgNS = "http://www.w3.org/2000/svg";

function makeNode(x: number, y: number, fill: string, rnd: () => number): SVGGElement {
  const g = document.createElementNS(svgNS, "g");
  g.setAttribute("transform", `translate(${x}, ${y})`);

  const body = document.createElementNS(svgNS, "rect");
  body.setAttribute("class", "n-body");
  body.setAttribute("width", String(NODE_W));
  body.setAttribute("height", String(NODE_H));
  body.setAttribute("rx", "6");
  body.setAttribute("fill", fill);
  g.appendChild(body);

  // The glyph mark: a small ink shape on the body, like the app's glyphs.
  const glyph = document.createElementNS(svgNS, rnd() < 0.5 ? "circle" : "rect");
  glyph.setAttribute("class", "n-glyph");
  if (glyph.tagName === "circle") {
    glyph.setAttribute("cx", "20");
    glyph.setAttribute("cy", String(NODE_H / 2));
    glyph.setAttribute("r", "6");
  } else {
    glyph.setAttribute("x", "14");
    glyph.setAttribute("y", String(NODE_H / 2 - 6));
    glyph.setAttribute("width", "12");
    glyph.setAttribute("height", "12");
    glyph.setAttribute("rx", "3");
  }
  g.appendChild(glyph);

  // A few faint "param" lines where the label would sit.
  for (let i = 0; i < 2; i++) {
    const line = document.createElementNS(svgNS, "rect");
    line.setAttribute("x", "36");
    line.setAttribute("y", String(10 + i * 8));
    line.setAttribute("width", String(30 + rnd() * 34));
    line.setAttribute("height", "3");
    line.setAttribute("rx", "1.5");
    line.setAttribute("fill", "#1f2937");
    line.setAttribute("opacity", "0.28");
    g.appendChild(line);
  }
  return g;
}

function makeHandle(cx: number, cy: number, color: string): SVGCircleElement {
  const c = document.createElementNS(svgNS, "circle");
  c.setAttribute("class", "n-handle");
  c.setAttribute("cx", String(cx));
  c.setAttribute("cy", String(cy));
  c.setAttribute("r", "3.5");
  c.setAttribute("fill", color);
  return c;
}

function makeEdge(from: StageNode, to: StageNode, color: string): SVGPathElement {
  const x1 = from.x + NODE_W;
  const y1 = from.y + NODE_H / 2;
  const x2 = to.x;
  const y2 = to.y + NODE_H / 2;
  const dx = Math.max(40, (x2 - x1) * 0.5);
  const p = document.createElementNS(svgNS, "path");
  p.setAttribute("class", "n-edge");
  p.setAttribute("d", `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`);
  p.setAttribute("stroke", color);
  return p;
}

/** Builds one randomized subnetwork and animates it into place. Resolves when
 * the build (plus a hold) finishes, so the caller can loop with a new seed. */
function buildNetwork(stage: SVGSVGElement, seed: number): Promise<void> {
  const rnd = mulberry32(seed);
  stage.textContent = "";
  const w = stage.clientWidth || 1200;
  const h = stage.clientHeight || 700;
  stage.setAttribute("viewBox", `0 0 ${w} ${h}`);

  // Columns like a subflow: sources feeding modifiers feeding a gather.
  const cols = w < 700 ? 3 : 4;
  const perCol = w < 700 ? 2 : 3;
  const nodes: StageNode[] = [];
  for (let c = 0; c < cols; c++) {
    const count = c === cols - 1 ? 1 : 1 + Math.floor(rnd() * perCol);
    for (let i = 0; i < count; i++) {
      const x = (w / (cols + 1)) * (c + 0.7) + (rnd() - 0.5) * 60;
      const y = (h / (count + 1)) * (i + 1) + (rnd() - 0.5) * 80 - NODE_H / 2;
      const fill = CATEGORY_FILLS[Math.floor(rnd() * CATEGORY_FILLS.length)];
      const el = makeNode(x, y, fill, rnd);
      nodes.push({ x, y, col: c, fill, el });
    }
  }

  // Wire every node to one target in the next column (a DAG by construction).
  const edges: { from: StageNode; to: StageNode; color: string; el: SVGPathElement }[] = [];
  for (const n of nodes) {
    if (n.col === cols - 1) continue;
    const targets = nodes.filter((t) => t.col === n.col + 1);
    if (targets.length === 0) continue;
    const to = targets[Math.floor(rnd() * targets.length)];
    const color = WIRE_COLORS[Math.floor(rnd() * WIRE_COLORS.length)];
    const el = makeEdge(n, to, color);
    edges.push({ from: n, to, color, el });
    n.el.appendChild(makeHandle(NODE_W, NODE_H / 2, color));
    to.el.appendChild(makeHandle(0, NODE_H / 2, color));
  }

  // Edges under nodes.
  for (const e of edges) stage.appendChild(e.el);
  for (const n of nodes) stage.appendChild(n.el);

  if (reducedMotion) {
    // A composed still: no build-up, no loop.
    return new Promise(() => {});
  }

  // The build-up: nodes rise in sequence, each edge draws once both of its
  // endpoints are in (MPW rise/reveal easings, applied via WAAPI).
  const nodeAt = new Map<StageNode, number>();
  nodes.forEach((n, i) => {
    const t = i * 320;
    nodeAt.set(n, t);
    n.el.style.opacity = "0";
    setTimeout(() => {
      n.el.style.opacity = "1";
      n.el.animate(
        [
          { transform: `translate(${n.x}px, ${n.y + 24}px)`, opacity: 0 },
          { transform: `translate(${n.x}px, ${n.y}px)`, opacity: 1 },
        ],
        { duration: 750, easing: "cubic-bezier(0.22, 1, 0.36, 1)", fill: "both" },
      );
    }, t);
  });
  let lastT = 0;
  for (const e of edges) {
    const t = Math.max(nodeAt.get(e.from) ?? 0, nodeAt.get(e.to) ?? 0) + 350;
    lastT = Math.max(lastT, t + 700);
    const len = e.el.getTotalLength();
    e.el.style.strokeDasharray = String(len);
    e.el.style.strokeDashoffset = String(len);
    e.el.style.opacity = "0";
    setTimeout(() => {
      e.el.style.opacity = "0.85";
      e.el.animate([{ strokeDashoffset: len }, { strokeDashoffset: 0 }], {
        duration: 700,
        easing: "cubic-bezier(0.21, 0.47, 0.32, 0.98)",
        fill: "both",
      });
    }, t);
  }

  // Hold the finished network, then hand back for a rebuild.
  return new Promise((resolve) => setTimeout(resolve, lastT + 4500));
}

async function runStage(): Promise<void> {
  const stage = document.getElementById("node-stage") as SVGSVGElement | null;
  if (!stage) return;
  let seed = 7;
  // Rebuild on resize with the same seed so the layout re-fits.
  let resizeTimer: number | undefined;
  window.addEventListener("resize", () => {
    window.clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(() => void buildNetwork(stage, seed), 250);
  });
  // The build loop: fade out, new seed, build again.
  for (;;) {
    await buildNetwork(stage, seed);
    const fade = stage.animate([{ opacity: 0.5 }, { opacity: 0 }], {
      duration: 800,
      easing: "ease-out",
    });
    await fade.finished.catch(() => {});
    seed = (seed * 48271) % 0x7fffffff;
    stage.animate([{ opacity: 0 }, { opacity: 0.5 }], { duration: 600, fill: "both" });
  }
}

function initReveals(): void {
  const els = document.querySelectorAll<HTMLElement>(".reveal");
  if (reducedMotion || !("IntersectionObserver" in window)) {
    els.forEach((el) => el.classList.add("in"));
    return;
  }
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) {
          (e.target as HTMLElement).classList.add("in");
          io.unobserve(e.target);
        }
      }
    },
    { threshold: 0.15 },
  );
  els.forEach((el) => io.observe(el));
}

function initChrome(): void {
  const rail = document.getElementById("rail-fill");
  const nav = document.querySelector<HTMLElement>(".nav");
  let ticking = false;
  const onScroll = () => {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(() => {
      const max = document.documentElement.scrollHeight - window.innerHeight;
      const p = max > 0 ? window.scrollY / max : 0;
      if (rail) rail.style.transform = `scaleX(${p})`;
      nav?.classList.toggle("scrolled", window.scrollY > 8);
      ticking = false;
    });
  };
  window.addEventListener("scroll", onScroll, { passive: true });
  onScroll();

  const year = document.getElementById("year");
  if (year) year.textContent = String(new Date().getFullYear());
}

initChrome();
initReveals();
void runStage();
