// Completions for the wrangle editor.
//
// Two halves, and they answer different questions. The STATIC half is the
// language: builtins, context queries, type keywords and clock variables,
// all drawn from the same exported lists `wrangle_lang_drift.rs` holds to
// the Rust grammar, so a builtin added in Rust cannot be missing here
// without failing the build.
//
// The LIVE half is the geometry: the `@attr` lanes actually present on the
// node's incoming geometry. That is what turns the language from something
// you memorise into something you discover, and it is why the completion
// source is built per node rather than being a constant.

import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import type { AttrLane } from "../../engine/types";
// From the names module, NOT from `wrangleLang`: that one pulls CodeMirror,
// and this file is reached from the eagerly-loaded parameter panel.
import { BUILTINS, LOCAL_TYPES, QUERIES, VARS } from "./wrangleNames";

/** One-line descriptions. Deliberately terse: a completion list is read at
 * a glance, and anything longer belongs in the wiki. */
const BUILTIN_DOC: Record<string, string> = {
  abs: "absolute value",
  sign: "-1, 0 or 1 (zero stays zero)",
  floor: "round down",
  ceil: "round up",
  round: "round to nearest",
  min: "smallest of two or more",
  max: "largest of two or more",
  clamp: "clamp(v, lo, hi)",
  fit: "fit(v, oldMin, oldMax, newMin, newMax)",
  lerp: "lerp(a, b, t)",
  sqrt: "square root",
  pow: "pow(base, exponent)",
  exp: "e to the power",
  log: "natural logarithm",
  sin: "sine, radians",
  cos: "cosine, radians",
  tan: "tangent, radians",
  asin: "arc sine",
  acos: "arc cosine",
  atan: "arc tangent",
  atan2: "atan2(y, x), full circle",
  radians: "degrees to radians",
  degrees: "radians to degrees",
  fmod: "remainder, keeps the sign of the dividend",
  rand: "deterministic random; shares the scatter node's generator",
  noise: "value noise, frozen so a scene renders identically forever",
  length: "vector magnitude",
  distance: "distance between two points",
  dot: "dot product",
  cross: "cross product of two vec3",
  normalize: "unit-length vector",
  set: "build a vector: set(x, y, z)",
};

const QUERY_DOC: Record<string, string> = {
  ch: 'read another node\'s parameter: ch("box1/width")',
  bbox: 'incoming bounds: bbox("xmin"), bbox("size")',
  npoints: "point count of the incoming geometry",
  nprims: "primitive count of the incoming geometry",
  nmeshes: "mesh count of the incoming geometry",
  centroid: "centre of the incoming bounds",
};

const VAR_DOC: Record<string, string> = {
  T: "scene seconds. Reading it makes the node recook every frame",
  F: "current frame. Reading it makes the node recook every frame",
  FPS: "frames per second",
  PI: "3.14159...",
  E: "2.71828...",
};

const TYPE_DOC: Record<string, string> = {
  float: "one component",
  vector2: "two components",
  vector: "three components",
  vector4: "four components",
};

/** The lanes every wrangle can read regardless of what the input carries. */
const IMPLICIT_LANES: { name: string; detail: string }[] = [
  { name: "P", detail: "position (vec3). Assign it to move geometry" },
  { name: "N", detail: "normal (vec3)" },
  { name: "Cd", detail: "colour (vec3). The lane the viewport already displays" },
  { name: "uv", detail: "texture coordinate (vec2)" },
  { name: "ptnum", detail: "this point's index" },
  { name: "numpt", detail: "point count" },
  { name: "primnum", detail: "this primitive's index" },
  { name: "numprim", detail: "primitive count" },
];

/** Static language completions, built once. */
const LANGUAGE: Completion[] = [
  ...BUILTINS.map((name) => ({
    label: name,
    type: "function",
    detail: BUILTIN_DOC[name] ?? "builtin",
    apply: `${name}(`,
  })),
  ...QUERIES.map((name) => ({
    label: name,
    type: "function",
    detail: QUERY_DOC[name] ?? "context query",
    apply: `${name}(`,
  })),
  ...LOCAL_TYPES.map((name) => ({
    label: name,
    type: "type",
    detail: TYPE_DOC[name] ?? "local declaration",
  })),
];

/** `$T` and friends, offered once the user has typed the `$`. */
const DOLLAR_VARS: Completion[] = VARS.map((name) => ({
  label: `$${name}`,
  type: "constant",
  detail: VAR_DOC[name] ?? "variable",
}));

/** Attribute completions for a lane inventory.
 *
 * The implicit lanes come first and are always offered; lanes the input
 * actually carries follow, tagged with their width so `@` completion doubles
 * as a readout of what is on the geometry. A lane the input carries that is
 * also implicit is not duplicated.
 */
export function attrCompletions(lanes: readonly AttrLane[]): Completion[] {
  const implicitNames = new Set(IMPLICIT_LANES.map((l) => l.name));
  return [
    ...IMPLICIT_LANES.map((l) => ({
      label: `@${l.name}`,
      type: "property",
      detail: l.detail,
    })),
    ...lanes
      .filter((l) => !implicitNames.has(l.name))
      .map((l) => ({
        label: `@${l.name}`,
        type: "property",
        detail: `${l.ty} on the input`,
      })),
  ];
}

/** Builds a completion source over a lane inventory.
 *
 * `lanes` is read through a getter rather than captured, because the
 * incoming geometry changes as the graph is edited and a completion list
 * frozen at editor-construction time would go stale the first time someone
 * wired something up.
 */
export function wrangleCompletions(getLanes: () => readonly AttrLane[]) {
  return (context: CompletionContext): CompletionResult | null => {
    // `@lane` and `$VAR` need their sigil in the match, or CodeMirror would
    // replace only the word after it and leave a stray `@`.
    const attr = context.matchBefore(/@\w*/);
    if (attr) {
      return { from: attr.from, options: attrCompletions(getLanes()) };
    }
    const dollar = context.matchBefore(/\$\w*/);
    if (dollar) {
      return { from: dollar.from, options: DOLLAR_VARS };
    }
    const word = context.matchBefore(/\w+/);
    // `explicit` is a deliberate Ctrl-Space: offer everything even with no
    // prefix typed, which is how someone with no idea what exists finds out.
    if (!word && !context.explicit) return null;
    return {
      from: word ? word.from : context.pos,
      options: LANGUAGE,
    };
  };
}
