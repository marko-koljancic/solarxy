// The About dialog's derived lines: the version must be the build-time
// package version (never a hardcoded string going stale between
// releases), and the copyright year must follow the clock.

import { describe, expect, it } from "vitest";
import pkg from "../../package.json";
import { DOC_LINKS, aboutCopyrightLine, aboutVersionLine, docUrl } from "./AboutModal";

describe("AboutModal lines", () => {
  it("renders the package version, not a hardcoded one", () => {
    expect(aboutVersionLine()).toBe(`Version ${pkg.version}`);
  });

  it("renders the copyright holder with the current year", () => {
    expect(aboutCopyrightLine(new Date("2026-07-23"))).toBe("© 2026 Marko Koljancic");
    expect(aboutCopyrightLine(new Date("2031-01-01"))).toBe("© 2031 Marko Koljancic");
  });
});

describe("AboutModal doc links", () => {
  // A wiki page's URL IS its filename with hyphens for spaces, so a label
  // typed into the page slot produces a 404 that nothing else would catch.
  it("points every link at a hyphenated wiki page path", () => {
    expect(DOC_LINKS.length).toBeGreaterThan(0);
    for (const { label, page } of DOC_LINKS) {
      expect(label.length).toBeGreaterThan(0);
      expect(page).toMatch(/^[A-Za-z0-9-]+$/);
      expect(page).not.toContain(" ");
      expect(docUrl(page)).toBe(
        `https://github.com/marko-koljancic/solarxy/wiki/${page}`,
      );
    }
  });

  it("names each capability once", () => {
    const pages = DOC_LINKS.map((l) => l.page);
    expect(new Set(pages).size).toBe(pages.length);
  });
});
