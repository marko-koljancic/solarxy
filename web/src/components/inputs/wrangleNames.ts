// The wrangle language's vocabulary, and nothing else.
//
// Deliberately free of every import, for the same reason `displayDefaults.ts`
// is: the completion source needs these names, the completion source is
// reached from the eagerly-loaded parameter panel, and the syntax
// highlighter beside them pulls CodeMirror. Keeping the lists here is what
// lets the editor stay in its own lazy chunk while the names travel freely.
//
// Anything added here must stay importable by a module that has no editor at
// all. `wrangle_lang_drift.rs` reads THIS file and fails the build if these
// lists and the Rust grammar disagree.

/** Pure functions: computed from their arguments alone.
 * Pinned to `solarxy_graph::expr::builtins::BUILTIN_NAMES`. */
export const BUILTINS = [
  "abs", "sign", "floor", "ceil", "round", "min", "max", "clamp", "fit",
  "lerp", "sqrt", "pow", "exp", "log", "sin", "cos", "tan", "asin", "acos",
  "atan", "atan2", "radians", "degrees", "fmod", "rand", "noise", "length",
  "distance", "dot", "cross", "normalize", "set",
] as const;

/** Context reads: resolved against the document, not computed.
 * Pinned to `solarxy_graph::expr::builtins::QUERY_NAMES`. */
export const QUERIES = ["ch", "bbox", "npoints", "nprims", "nmeshes", "centroid"] as const;

/** Local declaration keywords.
 * Pinned to `solarxy_graph::expr::builtins::LOCAL_TYPE_NAMES`. */
export const LOCAL_TYPES = ["float", "vector2", "vector", "vector4"] as const;

/** Clock and math constants, without their leading `$`.
 * Pinned to `solarxy_graph::expr::ast::Var::ALL`. */
export const VARS = ["T", "F", "FPS", "PI", "E"] as const;
