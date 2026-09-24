// The shortcuts reference is generated from the keymap table, and this is
// what says so. The selection used to sit inline in the component, where
// nothing could see it: a binding could have stopped being listed, or been
// listed twice, and the only way to notice was to open the modal and count.
//
// No DOM here. The two functions are pure and exported for exactly that
// reason, the way AboutModal exports its own.

import { describe, expect, it } from "vitest";
import { KEY_GROUPS, KEYMAP } from "../input/keymap";
import { shortcutGroups, shortcutNotes } from "./ShortcutsModal";

describe("the shortcuts reference", () => {
  it("lists every binding the table declares, exactly once", () => {
    const listed = shortcutGroups().flatMap((g) => g.bindings.map((b) => b.id));
    const declared = KEYMAP.map((b) => b.id);
    expect([...listed].sort()).toEqual([...declared].sort());
    expect(new Set(listed).size).toBe(listed.length);
  });

  it("orders its sections as the group list orders them", () => {
    const shown = shortcutGroups().map((g) => g.group);
    const expected = KEY_GROUPS.filter((g) => KEYMAP.some((b) => b.group === g));
    expect(shown).toEqual([...expected]);
  });

  it("draws no empty section", () => {
    for (const { group, bindings } of shortcutGroups()) {
      expect(bindings.length, `${group} is listed with nothing under it`).toBeGreaterThan(0);
    }
  });

  it("footnotes exactly the bindings that carry a note", () => {
    expect(shortcutNotes().map((b) => b.id)).toEqual(
      KEYMAP.filter((b) => b.note).map((b) => b.id),
    );
  });
});
