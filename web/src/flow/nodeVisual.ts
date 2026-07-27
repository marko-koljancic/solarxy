// The registry-driven node visual vocabulary: per-type glyphs and role
// silhouettes, resolved from the snapshot's `glyph` and `role` hints with
// a category fallback, so a node added in Rust renders with its declared
// identity and a node the frontend has never seen still renders sensibly
// (the zero-frontend-change contract). Glyph art is transplanted verbatim
// from the design source (solarxy/design/web/solarxy-web.pen, the
// Evo/Glyph set); every glyph is a 16x16 stroke path (round caps and
// joins, 1.5 width). Shaped bodies are generated 112x32 outlines:
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
  line: "M2.5 13L13.5 3 M7.1 9.6a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M10.8 6.3a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
  circle: "M9.8 3.66L12.34 6.2 12.34 9.8 9.8 12.34 6.2 12.34 3.66 9.8 3.66 6.2 6.2 3.66z",
  points_from_geo:
    "M8 3.5l4.5 8h-9z M8.9 3.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M13.4 11.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M4.4 11.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
  edges_to_geo: "M3.5 3.5h9v9h-9z M3.5 3.5l9 9 M12.5 3.5l-9 9",
  attribute_create: "M2.5 5h6l4.5 3-4.5 3h-6z M5 8h3 M6.5 6.5v3",
  attribute_randomize:
    "M3 3h10v10h-10z M6.4 5.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M8.9 8a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M11.4 10.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
  // Wrangle: a code window. The chevron and the caret read as "a program
  // runs here", which is the one thing that distinguishes this node from
  // every other attribute node at a glance.
  attribute_wrangle:
    "M2.5 3h11v10h-11z M2.5 5.4h11 M5.2 8l1.6 1.6-1.6 1.6 M8.4 11.2h2.6",
  // Promote: points rising into a primitive; Copy: the echoed-rect motif;
  // From Image: the image frame pouring down into geometry.
  attribute_promote:
    "M8 2.5l2.7 3.6h-5.4z M8 10.6V7.4m-1.5 1.5L8 7.4l1.5 1.5 M4.9 12.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M8.9 12.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M12.9 12.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
  attribute_copy: "M3 3h7v7h-7z M6 6h7v7h-7z",
  attribute_from_image:
    "M3 2.5h10v6h-10z M4.5 7l2-2.4 1.5 1.7 1.2-1.4 1.8 2.1 M8 9.5v3.5m-1.6-1.6L8 13l1.6-1.6",
  cone: "M8 3l4.5 8.3m-4.5-8.3l-4.5 8.3m9.2 0.7a4.7 1.7 0 1 1-9.4 0 4.7 1.7 0 1 1 9.4 0",
  cylinder:
    "M12.5 4.6a4.5 1.6 0 1 1-9 0 4.5 1.6 0 1 1 9 0m-9 0v6.8m9-6.8v6.8m0 0a4.5 1.6 0 1 0-9 0",
  torus: "M14 8a6 3.6 0 1 1-12 0 6 3.6 0 1 1 12 0m-3.5 0a2.5 1.3 0 1 1-5 0 2.5 1.3 0 1 1 5 0",
  torus_knot: "M10 8a3.5 3.5 0 1 1-7 0 3.5 3.5 0 1 1 7 0m3 0a3.5 3.5 0 1 1-7 0",
  transform: "M8 2v12m-6-6h12m-6-6l-1.8 1.8m1.8-1.8l1.8 1.8m4.2 4.2l-1.8-1.8m1.8 1.8l-1.8 1.8",
  // Displace: a surface bulging under an upward push.
  displace: "M2.5 11.5H5c1-3.5 5-3.5 6 0h2.5 M8 8V3.8m-1.6 1.6L8 3.8l1.6 1.6",
  mirror: "M8 2.5v11m-2.5-8.5l-2.5 3 2.5 3m5-6l2.5 3-2.5 3",
  array: "M2.5 9.5h4v4h-4z m3.5-3.5h4v4h-4z m3.5-3.5h4v4h-4z",
  scatter:
    "M2.5 12.5c2-2.5 9-2.5 11 0 M5.4 9.3a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M9 8.2a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M12.6 9.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M7 4.9a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M11.2 5.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
  copy_to_points:
    "M2.5 12.5c2-2.5 9-2.5 11 0 M3.3 7.6h2.4v2.4h-2.4z M6.8 4.8h2.4v2.4h-2.4z M10.3 7.6h2.4v2.4h-2.4z",
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

  // Texture-context generators: a swatch, a stop bar, a scatter, cells, a
  // radial fade, a checkerboard, a brick course.
  constant: "M3 3.5h10v9h-10z m6.8 4.5a1.8 1.8 0 1 1-3.6 0 1.8 1.8 0 1 1 3.6 0",
  ramp: "M2.5 5.5h11v3.5h-11z m1.5 5.5v2m4-2v2m4-2v2",
  noise:
    "M4.9 4.5a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M10.4 3.9a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M13.3 7.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M6.6 8.7a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M11 11.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0 M5.4 12.4a0.9 0.9 0 1 1-1.8 0 0.9 0.9 0 1 1 1.8 0",
  voronoi: "M3 3.5h10v9h-10z M8 8l1.5-4.5 M8 8l4 2 M8 8l-3 4.5",
  gradient:
    "M3 3.5h10v9h-10z M11 8a3 3 0 1 1-6 0 3 3 0 1 1 6 0 M9.3 8a1.3 1.3 0 1 1-2.6 0 1.3 1.3 0 1 1 2.6 0",
  checker: "M3 3.5h10v9h-10z M8 3.5v9 M3 8h10 M3.7 4.2l3.6 3.6 M8.7 8.7l3.6 3.3",
  brick:
    "M3 3.5h10v9h-10z M3 6.5h10 M3 9.5h10 M8 3.5v3 M5.5 6.5v3 M10.5 6.5v3 M8 9.5v3",

  // Texture-context adjustments: histogram, split disc, spoked wheel,
  // opposed arrows, the gamma letterform.
  levels: "M2.5 13.2h11m-9-0.2v-4m2.6 4v-7.5m2.6 7.5v-5.3m2.6 5.3v-8.7",
  brightness_contrast:
    "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M8 3v10 M10 6.2h1.6 M10 8h2.2 M10 9.8h1.6",
  hue_saturation: "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M8 8V3 M8 8l-4.33 2.5 M8 8l4.33 2.5",
  invert: "M3 5.5h8.5m0 0l-2-2m2 2l-2 2 M13 10.5h-8.5m0 0l2-2m-2 2l2 2",
  gamma: "M4.5 3.5c0.8 3.2 2 5.2 3.6 6.8m3.4-6.8c-0.2 4.6-1.6 7.6-4 9.7",

  // Texture-context composites: venn, echoed dot, peak, layer stack,
  // terrain-to-arrow.
  mix: "M9.4 8a3.4 3.4 0 1 1-6.8 0 3.4 3.4 0 1 1 6.8 0 M13.4 8a3.4 3.4 0 1 1-6.8 0",
  blur: "M10 8a2 2 0 1 1-4 0 2 2 0 1 1 4 0 M4.6 5.2a5 5 0 0 0 0 5.6 M11.4 5.2a5 5 0 0 1 0 5.6",
  sharpen: "M3.5 12.5l4.5-9 4.5 9z M8 8.2v4.3",
  pack_orm: "M8 2.5l5.5 3-5.5 3-5.5-3z m-5.5 5.7l5.5 3 5.5-3 m-11 2.6l5.5 3 5.5-3",
  height_to_normal:
    "M2.5 12.5c2-5 3.8-5 5.5-2s3.8 3 5.5-2 M11.5 8V3.5 m-1.7 1.7l1.7-1.7 1.7 1.7",

  // Containers: the geo body with a context motif inside.
  texnet:
    "M4.5 2.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z M5 10.5l2.2-2.7 1.6 1.8 1.4-1.6 1.8 2.5",
  matnet:
    "M4.5 2.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z M8 4.8c1.5 1.9 2.4 3 2.4 4.2a2.4 2.4 0 1 1-4.8 0c0-1.2 0.9-2.3 2.4-4.2z",

  // Material surfaces: the sphere family, one distinguishing mark each.
  principled: "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M7.3 6a1.3 1.3 0 1 1-2.6 0 1.3 1.3 0 1 1 2.6 0",
  matcap: "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M3.9 6.5a4.6 4.6 0 0 1 8.2 0",
  toon: "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M3.6 9.5h4.2l2-2.5h3 M5 11.8h3l1.8-2.2",
  unlit: "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M4.5 11.5l7-7",
  mix_material:
    "M6 4.5c1.2 1.5 2 2.6 2 3.7a2 2 0 1 1-4 0c0-1.1 0.8-2.2 2-3.7z M10.5 7c1.2 1.5 2 2.6 2 3.7a2 2 0 1 1-4 0c0-1.1 0.8-2.2 2-3.7z",
  tex_ref: "M6.5 4h7v8h-7z M8 10l1.8-2.2 1.6 1.9 M1.5 8h3.6m-1.5-1.6L5.3 8l-1.7 1.6",

  // Output: the import file motif reversed (arrow out), a frame with a
  // corner-out arrow, an aperture, a camera body.
  geo_export: "M4 2.5h5l3 3v8h-8z m5 0v3h3 M8 12v-4m-1.7 1.7L8 8l1.7 1.7",
  image_export:
    "M3 5.5h8v7.5h-8z M4.3 11.2l2-2.4 1.5 1.7 1.2-1.3 1.7 2 M10.5 2.5h3v3m0-3l-3.7 3.7",
  render: "M13 8a5 5 0 1 1-10 0 5 5 0 1 1 10 0 M8 3.6l3.8 6.6h-7.6z",
  camera:
    "M2.5 5.5h2.8l1.2-1.8h3l1.2 1.8h2.8v7h-11z M10.2 9a2.2 2.2 0 1 1-4.4 0 2.2 2.2 0 1 1 4.4 0",
};

/** Category -> the glyph shown when a node's declared key has no art (a
 * future Rust node with a novel glyph key degrades to its family icon). */
const CATEGORY_GLYPH: Record<NodeTypeSnapshot["category"], string> = {
  container: "geo",
  generators: "box",
  attribute: "attribute_create",
  transform: "transform",
  copy: "copy_to_points",
  topology: "subdivide",
  shaders: "material",
  import: "import_obj",
  export: "geo_export",
  lights: "point",
  utility: "null",
  tex_generate: "checker",
  tex_adjust: "levels",
  tex_composite: "mix",
};

/** Category -> the silhouette used when a node declares no role (older
 * snapshots, fabricated test nodes). */
const CATEGORY_ROLE: Record<NodeTypeSnapshot["category"], NodeRole> = {
  container: "container",
  generators: "standard",
  attribute: "standard",
  transform: "standard",
  copy: "standard",
  topology: "standard",
  shaders: "standard",
  import: "standard",
  export: "terminal",
  lights: "light",
  utility: "standard",
  tex_generate: "imageSource",
  tex_adjust: "standard",
  tex_composite: "standard",
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

/** Shaped 112x32 body outlines: every silhouette left-right
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
