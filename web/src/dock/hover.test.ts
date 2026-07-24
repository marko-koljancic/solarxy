import { beforeEach, describe, expect, it } from "vitest";
import { clearHoveredPanel, getHoveredPanel, setHoveredPanel } from "./hover";

describe("dock hover tracking", () => {
  beforeEach(() => {
    // Reset by clearing whatever is set.
    const current = getHoveredPanel();
    if (current) clearHoveredPanel(current);
  });

  it("starts with no hovered panel", () => {
    expect(getHoveredPanel()).toBeNull();
  });

  it("tracks the last entered panel", () => {
    setHoveredPanel("nodes");
    expect(getHoveredPanel()).toBe("nodes");
    setHoveredPanel("properties");
    expect(getHoveredPanel()).toBe("properties");
  });

  it("a stale leave does not clear a fresher enter", () => {
    // enter A, enter B (browser fired enter-B before leave-A), then leave A.
    setHoveredPanel("nodes");
    setHoveredPanel("properties");
    clearHoveredPanel("nodes");
    expect(getHoveredPanel()).toBe("properties");
  });

  it("a matching leave clears the hover", () => {
    setHoveredPanel("nodes");
    clearHoveredPanel("nodes");
    expect(getHoveredPanel()).toBeNull();
  });
});
