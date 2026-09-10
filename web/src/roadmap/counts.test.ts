// The roadmap data module publishes counts that describe things living
// somewhere else: the node-type count also appears in the landing page's
// stats band, and the crate count describes the workspace manifest. They are
// edited in different passes and have disagreed before, so each agreement is
// pinned here rather than left to the sync checklist alone.

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import { CRATE_COUNT, NODE_TYPE_COUNT } from "./data";

const here = dirname(fileURLToPath(import.meta.url));

it("the landing stats band and the roadmap data module agree on the node-type count", () => {
  const landing = readFileSync(resolve(here, "../../index.html"), "utf8");
  const m = landing.match(
    /<span class="stat-value">(\d+)<\/span>\s*<span class="stat-label">node types/,
  );
  expect(m, "the landing stats band no longer carries a node-type stat").toBeTruthy();
  expect(Number(m?.[1])).toBe(NODE_TYPE_COUNT);
});

// CRATE_COUNT said fifteen for three weeks against a sixteen-member
// workspace, and this file was already here when it happened: it pinned the
// node-type count and nothing pinned this one. A crate is added in a Rust
// pass that never opens the frontend, which is exactly the change a
// checklist entry does not survive, so the constant is held against the
// workspace manifest the way registry_drift.rs holds the README's count.
it("the roadmap data module agrees with the workspace on how many crates there are", () => {
  const manifest = readFileSync(resolve(here, "../../../Cargo.toml"), "utf8");
  const block = manifest.match(/members\s*=\s*\[([^\]]*)\]/);
  expect(block, "the workspace manifest no longer declares a members list").toBeTruthy();
  const members = (block?.[1] ?? "")
    .split(",")
    .map((entry) => entry.trim().replace(/^"|"$/g, ""))
    .filter((entry) => entry.length > 0);
  expect(members.length).toBe(CRATE_COUNT);
});
