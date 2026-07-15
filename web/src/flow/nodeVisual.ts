// The registry-driven node visual vocabulary (revamp R2, decisions D-4 and
// D-10..D-11, D-18; polish D-20/D-21): per-type glyphs and role
// silhouettes, resolved from the snapshot's `glyph` and `role` hints with
// a category fallback, so a node added in Rust renders with its declared
// identity and a node the frontend has never seen still renders sensibly
// (the zero-frontend-change contract). Glyph art is transplanted verbatim
// from the design source (solarxy/design/web/solarxy-web.pen, the
// Evo/Glyph set); every glyph is a 16x16 stroke path (round caps and
// joins, 1.5 width). Shaped bodies are generated 112x32 outlines (D-21):
// left-right symmetric vertex lists run through the corner-rounding
// helper below.

import type { NodeRole, NodeTypeSnapshot } from "../engine/types";

/** Glyph key -> 16x16 stroke path. Keys follow the Rust convention: the
 * type id, with lights dropping their `_light` suffix. The four model
 * importers deliberately share the file-import motif. */
export const GLYPH_PATHS: Record<string, string> = {
  box: "M3 5.5l5-2.5 5 2.5v5l-5 2.5-5-2.5z m0 0l5 2.5 5-2.5m-5 2.5v5",
  sphere: "M14 8a6 6 0 1 1-12 0 6 6 0 1 1 12 0m-11 2c1.5 1.1 8.5 1.1 10 0",
  plane: "M2 11l4-6h8l-4 6z",
  cone: "M8 3l4.5 8.3m-4.5-8.3l-4.5 8.3m9.2 0.7a4.7 1.7 0 1 1-9.4 0 4.7 1.7 0 1 1 9.4 0",
  cylinder:
    "M12.5 4.6a4.5 1.6 0 1 1-9 0 4.5 1.6 0 1 1 9 0m-9 0v6.8m9-6.8v6.8m0 0a4.5 1.6 0 1 0-9 0",
  torus: "M14 8a6 3.6 0 1 1-12 0 6 3.6 0 1 1 12 0m-3.5 0a2.5 1.3 0 1 1-5 0 2.5 1.3 0 1 1 5 0",
  torus_knot: "M10 8a3.5 3.5 0 1 1-7 0 3.5 3.5 0 1 1 7 0m3 0a3.5 3.5 0 1 1-7 0",
  transform: "M8 2v12m-6-6h12m-6-6l-1.8 1.8m1.8-1.8l1.8 1.8m4.2 4.2l-1.8-1.8m1.8 1.8l-1.8 1.8",
  mirror: "M8 2.5v11m-2.5-8.5l-2.5 3 2.5 3m5-6l2.5 3-2.5 3",
  array: "M2.5 9.5h4v4h-4z m3.5-3.5h4v4h-4z m3.5-3.5h4v4h-4z",
  subdivide: "M3 3h10v10h-10z m5 0v10m-5-5h10",
  compute_normals: "M2.5 12c2-2 9-2 11 0m-5.5-2.5v-6.5m0 0l-2 2m2-2l2 2",
  delete: "M3 4.5h10m-6.5 0v-1.5h3v1.5m-5 0l0.7 8.5h5.6l0.7-8.5",
  bounds: "M3 5.5v-2.5h2.5m5 0h2.5v2.5m0 5v2.5h-2.5m-5 0h-2.5v-2.5",
  uv_project: "M3 6h7v7h-7z m7 0c2.6 0 3.6-2 3.1-4m-6.6 4v-2m-3.5 5.5h7",
  material: "M8 2.5c2.5 3 4 4.8 4 6.8a4 4 0 1 1-8 0c0-2 1.5-3.8 4-6.8z m-1.2 7a1.3 1.3 0 1 0 2.6 0",
  merge: "M3 4h3.5l3.5 4h3m-10 4h3.5l3.5-4m3 0l-1.8-1.8m1.8 1.8l-1.8 1.8",
  switch: "M3 5h8m-8 6h8m0-6l2.5 3-2.5 3",
  null: "M11 8a3 3 0 1 1-6 0 3 3 0 1 1 6 0",
  validate: "M8 2l5 2v4c0 3-2.2 4.8-5 6-2.8-1.2-5-3-5-6v-4z m-2.2 6.2l1.6 1.6 3-3.4",
  import_obj: "M4 2.5h5l3 3v8h-8z m5 0v3h3m-4 1.5v4m-1.7-1.7l1.7 1.7 1.7-1.7",
  import_gltf: "M4 2.5h5l3 3v8h-8z m5 0v3h3m-4 1.5v4m-1.7-1.7l1.7 1.7 1.7-1.7",
  import_stl: "M4 2.5h5l3 3v8h-8z m5 0v3h3m-4 1.5v4m-1.7-1.7l1.7 1.7 1.7-1.7",
  import_ply: "M4 2.5h5l3 3v8h-8z m5 0v3h3m-4 1.5v4m-1.7-1.7l1.7 1.7 1.7-1.7",
  import_image:
    "M3 4h10v8h-10z m2.5 4.3a1.1 1.1 0 1 0 0-2.2m-2 5.4l3-3 2.3 2.3 2.2-2.3 2 2",
  geo: "M4.5 2.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z m1 5.5h5",
  note: "M3 3h10v6.5l-3.5 3.5h-6.5z m6.5 10v-3.5h3.5",
  ambient: "M11 8a3 3 0 1 1-6 0 3 3 0 1 1 6 0m-7.8-4.8l1 1m8.6-1l-1 1m-8.6 8.6l1-1m8.6 1l-1-1",
  directional:
    "M5 3v5m0 0l-1.6-1.6m1.6 1.6l1.6-1.6m2.9-3.4v5m0 0l-1.6-1.6m1.6 1.6l1.6-1.6m-8.6 5.6c2.2-1.6 8.8-1.6 11 0",
  hemisphere: "M2.5 11h11m-9.7 0a4.2 4.2 0 0 1 8.4 0",
  point:
    "M9.4 8a1.4 1.4 0 1 1-2.8 0 1.4 1.4 0 1 1 2.8 0m-1.4-5.2v2m0 6.4v2m-5.2-5.2h2m6.4 0h2m-8.9-3.7l1.4 1.4m4.6 4.6l1.4 1.4m0-7.4l-1.4 1.4m-4.6 4.6l-1.4 1.4",
  rect_area: "M3 3h6v10h-6z m8 2h2.5m-2.5 3h2.5m-2.5 3h2.5",
  spot: "M6.5 3h3l1.2 3.5h-5.4z m-2.5 10l2.3-6.5m5.7 6.5l-2.3-6.5m-5.7 6.5h8",
};

/** Category -> the glyph shown when a node's declared key has no art (a
 * future Rust node with a novel glyph key degrades to its family icon). */
const CATEGORY_GLYPH: Record<NodeTypeSnapshot["category"], string> = {
  container: "geo",
  primitives: "box",
  modifiers: "transform",
  import: "import_obj",
  lights: "point",
  utility: "null",
};

/** Category -> the silhouette used when a node declares no role (older
 * snapshots, fabricated test nodes). */
const CATEGORY_ROLE: Record<NodeTypeSnapshot["category"], NodeRole> = {
  container: "container",
  primitives: "standard",
  modifiers: "standard",
  import: "standard",
  lights: "light",
  utility: "standard",
};

/** The 16x16 glyph path for a node type: declared key first, category art
 * as the fallback. Always returns drawable art. */
export function glyphPath(desc: NodeTypeSnapshot | undefined): string {
  if (!desc) return GLYPH_PATHS.null;
  return GLYPH_PATHS[desc.glyph] ?? GLYPH_PATHS[CATEGORY_GLYPH[desc.category]];
}

/** Role values this frontend has silhouettes for; a NEWER Rust enum
 * variant arrives as an unknown string and falls back by category. */
const KNOWN_ROLES: ReadonlySet<string> = new Set([
  "standard",
  "container",
  "gather",
  "branch",
  "terminal",
  "analyzer",
  "imageSource",
  "light",
  "note",
]);

/** The silhouette family for a node type, with the category fallback. */
export function nodeRole(desc: NodeTypeSnapshot | undefined): NodeRole {
  if (!desc) return "standard";
  return KNOWN_ROLES.has(desc.role) ? desc.role : CATEGORY_ROLE[desc.category];
}

/** A polygon vertex with its corner radius: [x, y, r]. */
export type RoundedVertex = [number, number, number];

function fmt(v: number): string {
  return String(Math.round(v * 100) / 100);
}

/** A closed SVG path for a polygon with per-vertex rounded corners: each
 * corner enters and exits `r` along its adjacent edges (clamped to half
 * the edge so short edges cannot overshoot) and turns through a quadratic
 * curve at the vertex. Pure, so it is unit-testable. */
export function roundedPolygonPath(points: RoundedVertex[]): string {
  const n = points.length;
  const parts: string[] = [];
  for (let i = 0; i < n; i++) {
    const [px, py, r] = points[i];
    const [ax, ay] = points[(i + n - 1) % n];
    const [bx, by] = points[(i + 1) % n];
    const din = Math.hypot(px - ax, py - ay);
    const dout = Math.hypot(bx - px, by - py);
    const rIn = Math.min(r, din / 2);
    const rOut = Math.min(r, dout / 2);
    const inX = px + ((ax - px) / din) * rIn;
    const inY = py + ((ay - py) / din) * rIn;
    const outX = px + ((bx - px) / dout) * rOut;
    const outY = py + ((by - py) / dout) * rOut;
    parts.push(`${i === 0 ? "M" : "L"} ${fmt(inX)} ${fmt(inY)}`);
    parts.push(`Q ${fmt(px)} ${fmt(py)} ${fmt(outX)} ${fmt(outY)}`);
  }
  parts.push("Z");
  return parts.join(" ");
}

/** Shaped 112x32 body outlines (D-21): every silhouette left-right
 * symmetric with rounded corners. Roles absent here render as plain CSS
 * rectangles. */
export const ROLE_BODY_PATHS: Partial<Record<NodeRole, string>> = {
  // The two-way junction: a symmetric hexagon.
  branch: roundedPolygonPath([
    [0, 16, 5],
    [18, 0, 4],
    [94, 0, 4],
    [112, 16, 5],
    [94, 32, 4],
    [18, 32, 4],
  ]),
  // Wide intake, narrowed readout: a symmetric trapezoid.
  analyzer: roundedPolygonPath([
    [0, 0, 4],
    [112, 0, 4],
    [102, 32, 4],
    [10, 32, 4],
  ]),
  // A file ticket with both top corners chamfered (the old single-fold
  // motif was asymmetric).
  imageSource: roundedPolygonPath([
    [14, 0, 3],
    [98, 0, 3],
    [112, 14, 3],
    [112, 32, 4],
    [0, 32, 4],
    [0, 14, 3],
  ]),
  // A lamp dome (the old lampshade was asymmetric): heavy top rounding.
  light: roundedPolygonPath([
    [0, 0, 14],
    [112, 0, 14],
    [112, 32, 4],
    [0, 32, 4],
  ]),
};
