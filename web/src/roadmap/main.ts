/* Boot for the roadmap page: render every mount from the data module, start
 * the card explorer, then wire the shared chrome, the scroll spy, the
 * accordions, and the deep-link landing. The page is one long document with
 * no view switching; SECTIONS drives the nav table of contents so it can
 * never disagree with the section ids in the shell. */

import { initRail, initReveals, initTheme, initYear, reducedMotion } from "../public/chrome";
import {
  ARCH_LAYERS,
  BACKLOG_WAVES,
  CHANGELOG,
  COMMITMENTS,
  CONTRACTS,
  COVERAGE,
  COVERAGE_RELEASES,
  ENABLERS,
  FOOTER_META,
  HERO_CHIPS,
  JOURNEYS,
  ORDERING,
  PERSONAS,
  PROGRAM,
  PROGRAM_STATS,
  RELEASE_PLAN,
  RELEASES,
  SECTIONS,
  SHORTLIST,
  STATS,
  TIMELINE,
  UX_CONTRACT,
} from "./data";
import { initExplorer } from "./explorer";
import * as render from "./render";

function mount(id: string, html: string): void {
  const el = document.getElementById(id);
  if (el) el.innerHTML = html;
}

mount("navlinks", render.navToc(SECTIONS));
mount("heroChips", render.heroChips(HERO_CHIPS));
mount("statsBand", render.statsBand(STATS));
mount("archLayers", render.archLayers(ARCH_LAYERS));
mount("contractStrip", render.contractStrip(CONTRACTS));
mount("commitGrid", render.commitmentsGrid(COMMITMENTS));
mount("timeline", render.timelineList(TIMELINE));
mount("releases", render.releaseList(RELEASES));
mount("changelogOut", render.changelogList(CHANGELOG));
mount("releasePlan", render.releaseLadder(RELEASE_PLAN));
mount("programStats", render.statsBand(PROGRAM_STATS));
mount("spine", render.programSpine(PROGRAM));
mount("backlogWaves", render.backlogWaves(BACKLOG_WAVES));
mount("personaGrid", render.personaGrid(PERSONAS));
mount("journeys", render.journeyList(JOURNEYS));
mount("uxContract", render.uxContractList(UX_CONTRACT));
mount("coverageMatrix", render.coverageMatrix(COVERAGE_RELEASES, COVERAGE, JOURNEYS));
mount("enablers", render.enablerCards(ENABLERS));
mount("shortlist", render.shortlistList(SHORTLIST));
mount("ordering", render.orderingList(ORDERING));
mount("footerMeta", render.footerMeta(FOOTER_META));

/* The rail handler exists only after initRail below, but the explorer fires
 * its first re-render during boot, so it reaches the handler through a
 * mutable indirection. Every explorer re-render changes the document height,
 * which is exactly what the rail's progress fraction is computed from. */
let refreshRail = (): void => {};
initExplorer(() => refreshRail());

initTheme();
initReveals();
refreshRail = initRail();
initYear();

/* One delegated handler per accordion collection; the rendered heads are
 * buttons, so Enter and Space come free with click. */
function accordion(containerId: string, itemSel: string, headSel: string): void {
  const box = document.getElementById(containerId);
  if (!box) return;
  box.addEventListener("click", (e) => {
    const head = (e.target as HTMLElement).closest<HTMLElement>(headSel);
    if (!head || !box.contains(head)) return;
    const item = head.closest<HTMLElement>(itemSel);
    if (!item) return;
    const open = item.classList.toggle("open");
    head.setAttribute("aria-expanded", open ? "true" : "false");
    const t = item.querySelector(".toggle");
    if (t) t.textContent = open ? "-" : "+";
  });
}
accordion("timeline", ".tl-item", ".tl-head");
accordion("changelogOut", ".cl-item", ".cl-head");
accordion("journeys", ".jrow", ".jh");

/* Scroll spy over the section list, marking the active TOC link. */
const tocLinks = new Map<string, HTMLAnchorElement>();
for (const a of document.querySelectorAll<HTMLAnchorElement>("#navlinks a")) {
  tocLinks.set((a.getAttribute("href") ?? "").slice(1), a);
}
if ("IntersectionObserver" in window) {
  const spy = new IntersectionObserver(
    (entries) => {
      for (const en of entries) {
        if (!en.isIntersecting) continue;
        for (const link of tocLinks.values()) link.classList.remove("active");
        tocLinks.get(en.target.id)?.classList.add("active");
      }
    },
    { rootMargin: "-45% 0px -50% 0px" },
  );
  for (const s of SECTIONS) {
    const el = document.getElementById(s.id);
    if (el) spy.observe(el);
  }
}

/* The browser's native hash jump fired before the mounts were rendered, so a
 * deep link into data-rendered content landed short. Re-resolve it now that
 * the document has its full height. */
const hash = decodeURIComponent(window.location.hash.slice(1));
if (hash) {
  document.getElementById(hash)?.scrollIntoView({
    behavior: reducedMotion() ? "auto" : "smooth",
    block: "start",
  });
}
