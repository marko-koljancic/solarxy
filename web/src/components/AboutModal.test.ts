// The About dialog's derived lines: the version must be the build-time
// package version (never a hardcoded string going stale between
// releases), and the copyright year must follow the clock.

import { describe, expect, it } from "vitest";
import pkg from "../../package.json";
import { aboutCopyrightLine, aboutVersionLine } from "./AboutModal";

describe("AboutModal lines", () => {
  it("renders the package version, not a hardcoded one", () => {
    expect(aboutVersionLine()).toBe(`Version ${pkg.version}`);
  });

  it("renders the copyright holder with the current year", () => {
    expect(aboutCopyrightLine(new Date("2026-07-23"))).toBe("© 2026 Marko Koljancic");
    expect(aboutCopyrightLine(new Date("2031-01-01"))).toBe("© 2031 Marko Koljancic");
  });
});
