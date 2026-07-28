// The wrangle completion source. Tested through a fake CompletionContext
// rather than a live editor, because the interesting behaviour is entirely
// in what gets offered and from which position -- and the position is what
// decides whether accepting a completion leaves a stray `@` behind.

import { describe, expect, it } from "vitest";
import type { CompletionContext } from "@codemirror/autocomplete";
import type { AttrLane } from "../../engine/types";
import { attrCompletions, wrangleCompletions } from "./wrangleComplete";
import { BUILTINS, QUERIES, VARS } from "./wrangleNames";

/** A CompletionContext over `text`, with the cursor at its end. */
function contextAt(text: string, explicit = false): CompletionContext {
  return {
    pos: text.length,
    explicit,
    matchBefore(re: RegExp) {
      // Mirrors CodeMirror's contract: match the regex against the text
      // before the cursor, anchored to the cursor.
      const m = new RegExp(`(?:${re.source})$`).exec(text);
      return m ? { from: text.length - m[0].length, to: text.length, text: m[0] } : null;
    },
  } as unknown as CompletionContext;
}

const lanes: AttrLane[] = [
  { name: "density", ty: "float", len: 1 },
  { name: "P", ty: "vec3", len: 3 },
];
const source = wrangleCompletions(() => lanes);

describe("attrCompletions", () => {
  it("always offers the implicit lanes, even with nothing on the input", () => {
    const labels = attrCompletions([]).map((c) => c.label);
    // @P and @Cd exist on any wrangle regardless of what the input carries;
    // not offering them would be the completion list lying.
    expect(labels).toContain("@P");
    expect(labels).toContain("@Cd");
    expect(labels).toContain("@ptnum");
  });

  it("adds lanes the input actually carries, with their type", () => {
    const density = attrCompletions(lanes).find((c) => c.label === "@density");
    expect(density).toBeDefined();
    expect(density?.detail).toContain("float");
  });

  it("does not offer a lane twice when the input carries an implicit one", () => {
    // `P` is both implicit and present on the input here.
    const labels = attrCompletions(lanes).map((c) => c.label);
    expect(labels.filter((l) => l === "@P")).toHaveLength(1);
  });
});

describe("wrangleCompletions", () => {
  it("completes attributes from the @ itself, not from the word after it", () => {
    // The position matters: completing from after the `@` would leave a
    // stray sigil and produce `@@P`.
    const r = source(contextAt("@P = set(@de"));
    expect(r).not.toBeNull();
    expect(r!.from).toBe("@P = set(".length);
    expect(r!.options.map((o) => o.label)).toContain("@density");
  });

  it("completes clock variables from the $ itself", () => {
    const r = source(contextAt("float t = $"));
    expect(r!.from).toBe("float t = ".length);
    const labels = r!.options.map((o) => o.label);
    for (const v of VARS) expect(labels).toContain(`$${v}`);
  });

  it("offers the language for a bare word", () => {
    const r = source(contextAt("float d = len"));
    expect(r!.from).toBe("float d = ".length);
    const labels = r!.options.map((o) => o.label);
    expect(labels).toContain("length");
    // Every builtin and query the grammar accepts is offered, so a name
    // added in Rust cannot be silently missing from the list.
    for (const b of BUILTINS) expect(labels).toContain(b);
    for (const q of QUERIES) expect(labels).toContain(q);
  });

  it("opens a call for a function, so the paren is not typed twice", () => {
    const r = source(contextAt("len"));
    expect(r!.options.find((o) => o.label === "length")?.apply).toBe("length(");
    // A type keyword is not a call and must not gain one.
    expect(r!.options.find((o) => o.label === "float")?.apply).toBeUndefined();
  });

  it("stays quiet with no prefix unless asked explicitly", () => {
    // Popping a list of thirty names on every space would be unusable.
    expect(source(contextAt("@P = "))).toBeNull();
    // Ctrl-Space is how someone with no idea what exists finds out.
    expect(source(contextAt("@P = ", true))).not.toBeNull();
  });

  it("reads the lanes fresh on every request", () => {
    // The graph changes under the editor; a list captured at construction
    // would be stale the moment somebody wired something up.
    let current: AttrLane[] = [];
    const live = wrangleCompletions(() => current);
    expect(live(contextAt("@d"))!.options.map((o) => o.label)).not.toContain("@density");
    current = lanes;
    expect(live(contextAt("@d"))!.options.map((o) => o.label)).toContain("@density");
  });
});
