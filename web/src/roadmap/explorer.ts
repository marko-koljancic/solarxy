/* The merged card explorer: one grid over every roadmap card, with the
 * capability lenses (tier, impact, effort, fit, theme, critical path) and the
 * program lenses (release, disposition) in a single filter state. Release and
 * disposition are always derived from the program data via derivePlacement(),
 * never stored on a card, so the grid and the program spine cannot disagree.
 *
 * The search input's value is used only as a filter needle and is never
 * interpolated into markup; everything rendered comes from the data module. */

import { reducedMotion } from "../public/chrome";
import type { Card } from "./data";
import { CARDS, DISPOSITIONS, PROGRAM, THEMES } from "./data";
import { derivePlacement } from "./derive";
import { dispositionBand } from "./render";

const TIERS = ["Foundational", "Near", "Mid", "Long", "Aspirational"] as const;
const TIER_LABEL: Record<string, string> = {
  Foundational: "Foundational enabler",
  Near: "Near",
  Mid: "Mid",
  Long: "Long",
  Aspirational: "Aspirational",
};
const IMPACTS = ["High", "Medium", "Low"] as const;
const EFFORTS = ["S", "M", "L", "XL"] as const;
const FITS = ["Native", "Adaptable", "Structural", "Research"] as const;

const tierRank: Record<string, number> = {
  Foundational: 0,
  Near: 1,
  Mid: 2,
  Long: 3,
  Aspirational: 4,
};
const effortRank: Record<string, number> = {
  S: 0,
  "S-M": 1,
  M: 2,
  "M-L": 3,
  L: 4,
  "L-XL": 5,
  XL: 6,
};

const DISP_LABEL: Record<string, string> = Object.fromEntries(
  DISPOSITIONS.map((d) => [d.key, d.label]),
);

/* Release filter chips cover only where a card could still land: the
 * non-shipped program releases plus the two unscheduled buckets. */
const REL_FILTERS = PROGRAM.filter((r) => r.kind !== "shipped")
  .map((r) => r.v)
  .concat(["Backlog", "Deferred"]);

const { relOf, dispOf, trigOf } = derivePlacement();

function impactRank(s: string): number {
  if (s === "High" || s === "Med-High") return 3;
  if (s === "Medium" || s === "Low-Med") return 2;
  return 1;
}

/* "S-M" grades as S and M; "M + L" (the split desktop-wiring card) grades as
 * M and L, so a ranged effort matches either of its endpoints' chips. */
function effortTokens(s: string): string[] {
  return s.split(/[^A-Za-z]+/).filter((t) => t.length > 0);
}

interface ExplorerState {
  tier: Set<string>;
  impact: Set<string>;
  effort: Set<string>;
  fit: Set<string>;
  rel: Set<string>;
  disp: string | null;
  group: "theme" | "tier" | "path" | "release" | "disposition";
  q: string;
}

function chipRow(dim: string, values: readonly string[], labels?: Record<string, string>): string {
  return values
    .map(
      (v) =>
        `<button type="button" class="filter-chip" data-dim="${dim}" data-v="${v}" aria-pressed="false">${labels?.[v] ?? v}</button>`,
    )
    .join("");
}

function relLabels(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const v of REL_FILTERS) out[v] = /^\d/.test(v) ? `v${v}` : v;
  return out;
}

function controlsHTML(): string {
  const groups: [ExplorerState["group"], string][] = [
    ["theme", "Theme"],
    ["tier", "Tier"],
    ["path", "Critical path"],
    ["release", "Release"],
    ["disposition", "Disposition"],
  ];
  return `<div class="ctl-row">
      <div class="search"><span class="mono">/</span><input id="cardSearch" type="text" placeholder="Search cards" aria-label="Search roadmap cards"></div>
      <span class="ctl-label">Group</span>
      <div class="seg" id="groupSeg">${groups
        .map(
          ([g, label]) =>
            `<button type="button" data-g="${g}"${g === "theme" ? ' class="on"' : ""}>${label}</button>`,
        )
        .join("")}</div>
      <button type="button" class="reset" id="resetBtn">Reset</button>
    </div>
    <div class="ctl-row">
      <span class="ctl-label">Tier</span><div class="chips">${chipRow("tier", TIERS, TIER_LABEL)}</div>
      <span class="ctl-label">Impact</span><div class="chips">${chipRow("impact", IMPACTS)}</div>
    </div>
    <div class="ctl-row">
      <span class="ctl-label">Effort</span><div class="chips">${chipRow("effort", EFFORTS)}</div>
      <span class="ctl-label">Fit</span><div class="chips">${chipRow("fit", FITS)}</div>
    </div>
    <div class="ctl-row">
      <span class="ctl-label">Release</span><div class="chips">${chipRow("rel", REL_FILTERS, relLabels())}</div>
      <span class="count-note" id="countNote"></span>
    </div>`;
}

export function initExplorer(onRender: () => void): void {
  const controlsMount = document.getElementById("controls");
  const outMount = document.getElementById("explorerOut");
  const bandMount = document.getElementById("dispBand");
  if (!controlsMount || !outMount || !bandMount) return;
  /* Rebound so the non-null narrowing reaches the hoisted inner functions. */
  const controls = controlsMount;
  const out = outMount;
  const band = bandMount;

  const state: ExplorerState = {
    tier: new Set(),
    impact: new Set(),
    effort: new Set(),
    fit: new Set(),
    rel: new Set(),
    disp: null,
    group: "theme",
    q: "",
  };
  const dims: Record<string, Set<string>> = {
    tier: state.tier,
    impact: state.impact,
    effort: state.effort,
    fit: state.fit,
    rel: state.rel,
  };

  controls.innerHTML = controlsHTML();
  const search = controls.querySelector<HTMLInputElement>("#cardSearch");
  const countNote = controls.querySelector<HTMLElement>("#countNote");

  function matches(c: Card): boolean {
    if (state.tier.size && !state.tier.has(c.tier)) return false;
    if (state.impact.size) {
      const r = impactRank(c.impact);
      const ok = [...state.impact].some(
        (v) => (v === "High" && r === 3) || (v === "Medium" && r === 2) || (v === "Low" && r === 1),
      );
      if (!ok) return false;
    }
    if (state.effort.size) {
      const toks = effortTokens(c.effort);
      if (![...state.effort].some((v) => toks.includes(v))) return false;
    }
    if (state.fit.size && !state.fit.has(c.fit)) return false;
    const d = dispOf[c.id] ?? "backlog";
    if (state.disp && d !== state.disp) return false;
    if (state.rel.size) {
      const rs = relOf[c.id] ?? [];
      const ok = [...state.rel].some((v) =>
        v === "Backlog"
          ? d === "backlog"
          : v === "Deferred"
            ? d === "deferred" || d === "wont"
            : rs.includes(v),
      );
      if (!ok) return false;
    }
    if (state.q) {
      const hay = `${c.id} ${c.title} ${c.what} ${c.why}`.toLowerCase();
      if (!hay.includes(state.q)) return false;
    }
    return true;
  }

  function badges(c: Card): string {
    const bar = [1, 2, 3]
      .map((i) => `<i class="${impactRank(c.impact) >= i ? "on" : ""}"></i>`)
      .join("");
    const effCls = c.effort.includes("XL") ? " effort-xl" : "";
    return `<div class="grades">
      <span class="g g-impact" title="Impact ${c.impact}"><span class="bar">${bar}</span> <b>${c.impact}</b></span>
      <span class="g${effCls}">Effort <b>${c.effort}</b></span>
      <span class="g fit-${c.fit}">${c.fit}</span>
    </div>`;
  }

  function placementBadges(c: Card): string {
    const rs = relOf[c.id] ?? [];
    const d = dispOf[c.id] ?? "backlog";
    if (d === "shipped")
      return rs.map((v) => `<span class="ships-badge">Shipped v${v}</span>`).join("");
    if (rs.length) return rs.map((v) => `<span class="planned-badge">v${v}</span>`).join("");
    return `<span class="disp-chip d-${d}">${DISP_LABEL[d] ?? d}</span>`;
  }

  function cardHTML(c: Card): string {
    const en = c.en ? `<span class="enabler-badge">Enabler ${c.en}</span>` : "";
    const split = c.split
      ? `<div class="row"><span class="k">Ships in halves</span>${c.split.join("; ")}</div>`
      : "";
    const trig = trigOf[c.id]
      ? `<div class="row"><span class="k">Schedule when</span>${trigOf[c.id]}</div>`
      : "";
    return `<div class="card" tabindex="0" role="button" aria-expanded="false" data-id="${c.id}">
      <div class="top"><span class="cid">${c.id}</span><span class="tier tier-${c.tier}">${TIER_LABEL[c.tier]}</span>${en}${placementBadges(c)}</div>
      <h4>${c.title}</h4>
      ${badges(c)}
      <div class="what">${c.what}</div>
      <div class="detail">
        ${split}${trig}
        <div class="row"><span class="k">Why it matters</span>${c.why}</div>
        <div class="row"><span class="k">Dependencies</span>${c.dep}</div>
        <div class="row"><span class="k">Risks</span>${c.risk}</div>
      </div>
      <span class="expand">Details +</span>
    </div>`;
  }

  function groupHead(gn: string, title: string, meta: string): string {
    return `<div class="group-head"><span class="gn">${gn}</span><h3>${title}</h3><span class="gmeta">${meta}</span></div>`;
  }
  function cardsGrid(cards: Card[]): string {
    return `<div class="cards">${cards.map(cardHTML).join("")}</div>`;
  }
  function countLabel(n: number): string {
    return `${n} card${n === 1 ? "" : "s"}`;
  }
  const byId = (a: Card, b: Card): number =>
    a.id.localeCompare(b.id, undefined, { numeric: true });

  function renderGroups(list: Card[]): string {
    let html = "";
    if (state.group === "theme") {
      for (const t of THEMES) {
        const g = list.filter((c) => c.t === t.key);
        if (!g.length) continue;
        html += groupHead(t.num, t.title, countLabel(g.length));
        html += `<p class="group-blurb">${t.blurb}</p>`;
        html += cardsGrid(g);
      }
    } else if (state.group === "tier") {
      for (const tier of TIERS) {
        const g = list
          .filter((c) => c.tier === tier)
          .sort((a, b) => impactRank(b.impact) - impactRank(a.impact) || byId(a, b));
        if (!g.length) continue;
        html += groupHead(
          `<span class="tier tier-${tier}">${TIER_LABEL[tier]}</span>`,
          "",
          countLabel(g.length),
        );
        html += cardsGrid(g);
      }
    } else if (state.group === "path") {
      const score = (c: Card): number =>
        (tierRank[c.tier] ?? 4) * 100 + (3 - impactRank(c.impact)) * 10 + (effortRank[c.effort] ?? 3);
      const g = list.slice().sort((a, b) => score(a) - score(b) || byId(a, b));
      html += groupHead("Ranked", "By critical-path priority", countLabel(g.length));
      html += `<p class="group-blurb">Sorted by tier, then impact, then effort: foundational enablers and high-impact quick wins first, research spikes last. A priority signal, not a committed schedule.</p>`;
      html += cardsGrid(g);
    } else if (state.group === "release") {
      for (const r of PROGRAM) {
        const g = list.filter((c) => (relOf[c.id] ?? []).includes(r.v));
        if (!g.length) continue;
        html += groupHead(`v${r.v}`, r.code, countLabel(g.length));
        html += `<p class="group-blurb">${r.theme}</p>`;
        html += cardsGrid(g);
      }
      const rest = list.filter((c) => !(relOf[c.id] ?? []).length);
      if (rest.length) {
        html += groupHead("Unscheduled", "Backlog, deferred, and won't-do", countLabel(rest.length));
        html += `<p class="group-blurb">Each backlog card names the trigger that would schedule it; deferrals carry a rationale.</p>`;
        html += cardsGrid(rest);
      }
    } else {
      for (const d of DISPOSITIONS) {
        const g = list.filter((c) => (dispOf[c.id] ?? "backlog") === d.key);
        if (!g.length) continue;
        html += groupHead(String(d.n), d.label, `${g.length} shown`);
        html += `<p class="group-blurb">${d.blurb}</p>`;
        html += cardsGrid(g);
      }
    }
    return html;
  }

  function render(): void {
    const list = CARDS.filter(matches);
    const spans =
      state.group === "release"
        ? list.filter((c) => (relOf[c.id] ?? []).length > 1).length
        : 0;
    if (countNote) {
      countNote.textContent =
        `${list.length} of ${CARDS.length} cards` +
        (spans ? `, ${spans} spans two releases, shown in each` : "");
    }
    out.innerHTML = list.length
      ? renderGroups(list)
      : `<div class="empty">No cards match these filters. <button type="button" class="reset" data-act="clear">Clear</button></div>`;
    onRender();
  }

  function drawBand(): void {
    band.innerHTML = dispositionBand(DISPOSITIONS, state.disp);
  }

  function syncChips(): void {
    for (const chip of controls.querySelectorAll<HTMLElement>(".filter-chip")) {
      const set = dims[chip.dataset.dim ?? ""];
      const on = set ? set.has(chip.dataset.v ?? "") : false;
      chip.classList.toggle("on", on);
      chip.setAttribute("aria-pressed", on ? "true" : "false");
    }
  }

  function syncSeg(): void {
    for (const b of controls.querySelectorAll<HTMLElement>("#groupSeg button")) {
      b.classList.toggle("on", b.dataset.g === state.group);
    }
  }

  function resetAll(): void {
    state.tier.clear();
    state.impact.clear();
    state.effort.clear();
    state.fit.clear();
    state.rel.clear();
    state.disp = null;
    state.q = "";
    state.group = "theme";
    if (search) search.value = "";
    syncChips();
    syncSeg();
    drawBand();
    render();
  }

  function scrollToControls(): void {
    controls.scrollIntoView({ behavior: reducedMotion() ? "auto" : "smooth", block: "start" });
  }

  function toggleCard(card: HTMLElement): void {
    const open = card.classList.toggle("open");
    card.setAttribute("aria-expanded", open ? "true" : "false");
    const ex = card.querySelector(".expand");
    if (ex) ex.textContent = open ? "Details -" : "Details +";
  }

  controls.addEventListener("click", (e) => {
    const el = e.target as HTMLElement;
    const chip = el.closest<HTMLElement>(".filter-chip");
    if (chip) {
      const set = dims[chip.dataset.dim ?? ""];
      if (!set) return;
      const v = chip.dataset.v ?? "";
      if (set.has(v)) set.delete(v);
      else set.add(v);
      /* Release and disposition are near-duplicate lenses on the same axis,
       * so picking a release clears any disposition tile and vice versa. */
      if (chip.dataset.dim === "rel" && state.disp) {
        state.disp = null;
        drawBand();
      }
      syncChips();
      render();
      return;
    }
    const seg = el.closest<HTMLElement>("#groupSeg button");
    if (seg) {
      state.group = (seg.dataset.g as ExplorerState["group"] | undefined) ?? "theme";
      syncSeg();
      render();
      return;
    }
    if (el.closest("#resetBtn")) resetAll();
  });

  search?.addEventListener("input", () => {
    state.q = search.value.trim().toLowerCase();
    render();
  });

  out.addEventListener("click", (e) => {
    const el = e.target as HTMLElement;
    if (el.closest('[data-act="clear"]')) {
      resetAll();
      return;
    }
    const card = el.closest<HTMLElement>(".card");
    if (card) toggleCard(card);
  });
  out.addEventListener("keydown", (e) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    const card = (e.target as HTMLElement).closest<HTMLElement>(".card");
    if (card) {
      e.preventDefault();
      toggleCard(card);
    }
  });

  band.addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLElement>(".db");
    if (!b) return;
    const key = b.dataset.disp ?? null;
    state.disp = state.disp === key ? null : key;
    state.rel.clear();
    syncChips();
    drawBand();
    render();
    scrollToControls();
  });

  /* Card-id chips anywhere on the page (the spine, the backlog waves, the
   * enabler cards) open that card in the explorer. */
  document.addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLElement>(".sp-card");
    if (!b) return;
    const id = b.dataset.card ?? "";
    if (!id) return;
    resetAll();
    if (search) search.value = id;
    state.q = id.toLowerCase();
    render();
    scrollToControls();
  });

  drawBand();
  render();
}
