// Syntax highlighting for the wrangle language.
//
// The vocabulary here is NOT independently authored: it mirrors what the
// Rust grammar accepts, and `crates/solarxy-core/tests/tokens_drift.rs`
// fails the build if the two disagree. That matters because the cost of
// drift is silent and asymmetric -- a builtin added in Rust and missed here
// renders as a plain identifier, which reads as "this function does not
// exist" to the person typing it.
//
// A StreamLanguage rather than a Lezer grammar: the language is small
// (statements, assignment, calls, no control flow), and a token-level
// tokenizer is what the highlighting actually needs. A real parse tree would
// buy folding and indentation for a language with neither blocks nor nesting
// beyond parentheses.

import { StreamLanguage, type StreamParser } from "@codemirror/language";
import { tags } from "@lezer/highlight";

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

const BUILTIN_SET: ReadonlySet<string> = new Set(BUILTINS);
const QUERY_SET: ReadonlySet<string> = new Set(QUERIES);
const TYPE_SET: ReadonlySet<string> = new Set(LOCAL_TYPES);
const VAR_SET: ReadonlySet<string> = new Set(VARS);

/** The token name for one word, or null when it is an ordinary identifier
 * (a local, or a name that does not exist). Exported so the mapping is
 * testable without instantiating an editor. */
export function classifyWord(word: string): string | null {
  if (TYPE_SET.has(word)) return "typeName";
  if (QUERY_SET.has(word)) return "queryName";
  if (BUILTIN_SET.has(word)) return "builtinName";
  return null;
}

const wrangleParser: StreamParser<unknown> = {
  name: "wrangle",

  token(stream) {
    if (stream.eatSpace()) return null;

    // Comments: `//` to end of line, and `/* ... */` on one line. The Rust
    // parser accepts both, so the editor must not paint them as operators.
    if (stream.match("//")) {
      stream.skipToEnd();
      return "comment";
    }
    if (stream.match("/*")) {
      while (!stream.eol()) {
        if (stream.match("*/")) break;
        stream.next();
      }
      return "comment";
    }

    // `@attr` element scope, including the swizzle: `@P.x` highlights whole,
    // because the component is part of the reference, not a field access on
    // some other value.
    if (stream.match(/^@[A-Za-z_][A-Za-z0-9_]*(\.[xyzw]+)?/)) return "attributeName";

    // `$T` and friends. An unknown `$NAME` deliberately gets no highlight:
    // the parser rejects it, and painting it like a real variable would
    // hide the mistake until cook time.
    const dollar = stream.match(/^\$([A-Za-z_][A-Za-z0-9_]*)/) as RegExpMatchArray | null;
    if (dollar) return VAR_SET.has(dollar[1]) ? "variableName" : null;

    // Numbers, including a leading dot and an exponent.
    if (stream.match(/^(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?/)) return "number";

    // Strings: `ch("path/param")` is the common case.
    if (stream.match(/^"(?:[^"\\]|\\.)*"?/)) return "string";

    const word = stream.match(/^[A-Za-z_][A-Za-z0-9_]*/) as RegExpMatchArray | null;
    if (word) return classifyWord(word[0]);

    if (stream.match(/^(?:[+\-*/%]|==|!=|<=|>=|[<>=])/)) return "operator";
    if (stream.match(/^[(),;[\]]/)) return "punctuation";

    stream.next();
    return null;
  },

  languageData: {
    commentTokens: { line: "//", block: { open: "/*", close: "*/" } },
    closeBrackets: { brackets: ["(", "[", '"'] },
  },

  tokenTable: {
    // Token names above that are not standard CodeMirror ones need a tag,
    // or they paint as nothing at all.
    attributeName: tags.attributeName,
    builtinName: tags.function(tags.standard(tags.variableName)),
    queryName: tags.special(tags.variableName),
    typeName: tags.typeName,
    variableName: tags.constant(tags.variableName),
  },
};

export const wrangleLanguage = StreamLanguage.define(wrangleParser);
