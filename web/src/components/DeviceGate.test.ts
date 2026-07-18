// The device gate's pure decision function (item 10, "permissive" variant):
// fine pointers always enter; coarse pointers block only below the smallest
// width and get one warning in the middle band.

import { describe, expect, it } from "vitest";
import { gateLevel } from "./DeviceGate";

describe("gateLevel", () => {
  it("fine pointers always enter, at any width", () => {
    expect(gateLevel(320, false)).toBe("ok");
    expect(gateLevel(800, false)).toBe("ok");
    expect(gateLevel(2560, false)).toBe("ok");
  });

  it("blocks only the smallest coarse-pointer screens", () => {
    expect(gateLevel(320, true)).toBe("blocked");
    expect(gateLevel(559, true)).toBe("blocked");
  });

  it("warns on small-but-usable coarse screens", () => {
    expect(gateLevel(560, true)).toBe("warn");
    expect(gateLevel(899, true)).toBe("warn");
  });

  it("tablets and larger coarse screens enter untouched", () => {
    expect(gateLevel(900, true)).toBe("ok");
    expect(gateLevel(1280, true)).toBe("ok");
  });

  it("rotating a phone to landscape can unblock it", () => {
    // A 720x540-ish phone: blocked portrait, warned landscape.
    expect(gateLevel(540, true)).toBe("blocked");
    expect(gateLevel(720, true)).toBe("warn");
  });
});
