// Every hint a menu shows is asked of the keymap, never typed beside the
// entry.
//
// Typing them is how a hint and a binding come apart, and by 0.10.0 three
// had: Set Display Flag showed no key though one was bound, Maximize Panel
// showed a gesture the table does not give it, and Open Scene advertised a
// key nothing bound at all. Reading the table cannot produce any of those,
// and this is what keeps the next entry from going back to a literal.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { KEYMAP, menuHint } from "../../input/keymap";

const here = new URL(".", import.meta.url).pathname;

/** Every component that draws a menu entry with a hint beside it. */
const MENUS = [
  "../ViewportMenuBar.tsx",
  "../TextPane.tsx",
  "./MenuBar.tsx",
  "./NodePaneViewMenu.tsx",
  "./NodesMenu.tsx",
  "./PropertiesMenus.tsx",
];

function source(rel: string): string {
  return readFileSync(resolve(here, rel), "utf8");
}

describe("menu hints", () => {
  it("are never typed by hand", () => {
    const offenders: string[] = [];
    for (const rel of MENUS) {
      for (const line of source(rel).split("\n")) {
        // A quoted or interpolated value where a binding id belongs.
        if (/shortcut:\s*(["'`])/.test(line)) offenders.push(`${rel}: ${line.trim()}`);
      }
    }
    expect(offenders, "ask menuHint for these instead").toEqual([]);
  });

  it("name a binding the table actually declares", () => {
    const ids = new Set(KEYMAP.map((b) => b.id));
    const named: string[] = [];
    for (const rel of MENUS) {
      for (const m of source(rel).matchAll(/menuHint\("([^"]+)"\)/g)) named.push(m[1]);
    }
    expect(named.length, "the reader found no hints, so it is broken").toBeGreaterThan(20);
    for (const id of named) {
      expect(ids.has(id), `no binding is declared with the id ${id}`).toBe(true);
    }
  });

  it("render the key, so an entry never shows an empty hint", () => {
    for (const b of KEYMAP) {
      expect(menuHint(b.id), `${b.id} renders nothing`).not.toBe("");
    }
    expect(menuHint("no-such-binding")).toBe("");
  });
});
