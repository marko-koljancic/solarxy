// The roadmap page ships every string in this directory to the public site,
// and the Rust comment sweep deliberately ignores string literals, so this is
// the load-bearing check that authored content obeys the public-surface
// rules. It reads raw source text rather than imports, so string literals,
// comments, and markup are all covered.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const webRoot = resolve(here, "../..");
const repoRoot = resolve(webRoot, "..");

function sweptFiles(): Map<string, string> {
  const out = new Map<string, string>();
  const dirs = [here, resolve(webRoot, "src/references")];
  for (const dir of dirs) {
    if (!existsSync(dir)) continue;
    for (const name of readdirSync(dir)) {
      if (name.endsWith(".test.ts")) continue;
      if (!/\.(ts|css|html)$/.test(name)) continue;
      out.set(join(dir, name), readFileSync(join(dir, name), "utf8"));
    }
  }
  for (const page of ["roadmap.html", "references.html"]) {
    const path = resolve(webRoot, page);
    if (existsSync(path)) out.set(path, readFileSync(path, "utf8"));
  }
  return out;
}

const files = sweptFiles();

function offenders(pattern: RegExp): string[] {
  const hits: string[] = [];
  for (const [path, text] of files) {
    const lines = text.split("\n");
    for (let i = 0; i < lines.length; i += 1) {
      const m = lines[i].match(pattern);
      if (m) hits.push(`${path}:${i + 1} \`${m[0]}\``);
    }
  }
  return hits;
}

describe("public roadmap sources", () => {
  it("cover the data module and derivation at minimum", () => {
    const names = [...files.keys()];
    expect(names.some((p) => p.endsWith("data.ts"))).toBe(true);
    expect(names.some((p) => p.endsWith("derive.ts"))).toBe(true);
  });

  it("carry no internal planning codes", () => {
    for (const pattern of [
      /\b[A-Z]-\d+\b/,
      /\b(?:phase|stage|milestone)s?[ -]\d/i,
      /\bdecision[ -]?\d/i,
    ]) {
      expect(offenders(pattern)).toEqual([]);
    }
  });

  it("carry no internal document paths", () => {
    for (const pattern of [/\bDocs\//, /SOLARXY-[A-Z0-9][A-Z0-9.-]*\.md/]) {
      expect(offenders(pattern)).toEqual([]);
    }
  });

  it("carry no banned glyphs", () => {
    for (const pattern of [
      /—/,
      /–/,
      /[←-⇿➡⬅-⬇∞]/,
      /·/,
      /&(?:middot|mdash|ndash|rarr|larr|darr|uarr|infin);/,
      /[\u{1f000}-\u{1faff}\u{2700}-\u{27bf}\u{fe0f}]/u,
    ]) {
      expect(offenders(pattern)).toEqual([]);
    }
  });

  it("never write the traced-renderer reference as one word", () => {
    expect(offenders(/pathtracer/i)).toEqual([]);
  });

  // The names that must never appear are themselves secret, so they cannot be
  // hardcoded here: they are derived from the reference-checkout directory,
  // which exists only on the maintainer's machine. In CI this check skips and
  // the pattern checks above still run; the pre-release grep is authoritative.
  it("name no reference checkout other than the sanctioned prototype", () => {
    const refs = resolve(repoRoot, "../../References");
    if (!existsSync(refs)) return;
    const banned = readdirSync(refs).filter(
      (name) => !name.startsWith(".") && name.toLowerCase() !== "minimystx",
    );
    for (const name of banned) {
      const needle = name.toLowerCase();
      for (const [path, text] of files) {
        expect(
          text.toLowerCase().includes(needle),
          `${path} names a reference checkout`,
        ).toBe(false);
      }
    }
  });
});
