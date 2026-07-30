// The landing page's stats band and the roadmap data module both publish the
// node-type count. They live in different files, are edited in different
// passes, and have disagreed with each other before, so the agreement is
// pinned here rather than left to the sync checklist alone.

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import { NODE_TYPE_COUNT } from "./data";

const here = dirname(fileURLToPath(import.meta.url));

it("the landing stats band and the roadmap data module agree on the node-type count", () => {
  const landing = readFileSync(resolve(here, "../../index.html"), "utf8");
  const m = landing.match(
    /<span class="stat-value">(\d+)<\/span>\s*<span class="stat-label">node types/,
  );
  expect(m, "the landing stats band no longer carries a node-type stat").toBeTruthy();
  expect(Number(m?.[1])).toBe(NODE_TYPE_COUNT);
});
