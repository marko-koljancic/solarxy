/* Pure HTML renderers for the roadmap page: each function maps one collection
 * from the data module to a markup string and touches no DOM. main.ts owns
 * the mounts; explorer.ts owns the interactive card grid and reuses the
 * disposition band renderer here so the band has one markup source. The data
 * is hand-authored and swept by data.redaction.test.ts, so values are
 * interpolated as-is; nothing user-typed ever reaches these functions. */

import type {
  ArchLayer,
  BacklogWave,
  ChangelogEntry,
  Commitment,
  Disposition,
  DocEntry,
  Enabler,
  Journey,
  Persona,
  ProgramEntry,
  ReleasePlanEntry,
  ReleaseSummary,
  Section,
  ShortlistItem,
  Stat,
  TimelineEntry,
} from "./data";

export function navToc(sections: Section[]): string {
  return sections.map((s) => `<a href="#${s.id}">${s.label}</a>`).join("");
}

export function heroChips(chips: string[]): string {
  return chips.map((c) => `<span class="chip">${c}</span>`).join("");
}

export function statsBand(stats: Stat[]): string {
  return stats
    .map(
      (s) =>
        `<div class="stat"><span class="n">${s.n}</span><span class="l">${s.l}</span></div>`,
    )
    .join("");
}

export function archLayers(layers: ArchLayer[]): string {
  return layers
    .map(
      (l) => `<div class="layer">
      <div class="lh"><span class="mono">${l.tag}</span><span class="d">${l.desc}</span></div>
      <div class="crate-grid">${l.crates
        .map(
          (c) =>
            `<div class="crate ${l.cls}"><div class="cn">${c.n}</div><div class="cd">${c.d}</div></div>`,
        )
        .join("")}</div>
    </div>`,
    )
    .join("");
}

export function contractStrip(contracts: Stat[]): string {
  return contracts
    .map(
      (c) =>
        `<div class="contract"><div class="n">${c.n}</div><div class="l">${c.l}</div></div>`,
    )
    .join("");
}

export function commitmentsGrid(commitments: Commitment[]): string {
  return commitments
    .map((c) => `<div class="commit"><h4>${c.h}</h4><p>${c.p}</p></div>`)
    .join("");
}

export function timelineList(entries: TimelineEntry[]): string {
  return entries
    .map(
      (t) => `<div class="tl-item">
      <button type="button" class="tl-head" aria-expanded="false">
        <span class="date">${t.date}</span><span class="h">${t.h}</span><span class="toggle">+</span>
      </button>
      <div class="tl-body">${t.body}</div>
    </div>`,
    )
    .join("");
}

export function releaseList(releases: ReleaseSummary[]): string {
  return releases
    .map(
      (r) => `<div class="rel-item">
      <div class="rel-head"><span class="rel-v">${r.v}</span><span class="rel-st">${r.st}</span></div>
      <p>${r.p}</p>
    </div>`,
    )
    .join("");
}

export function changelogList(entries: ChangelogEntry[]): string {
  return entries
    .map((c) => {
      const groups = c.groups
        .map(
          (g) => `<div class="cl-group">
          <h5>${g.h}</h5>
          <ul>${g.items.map((i) => `<li><b>${i.lead}</b> ${i.text}</li>`).join("")}</ul>
        </div>`,
        )
        .join("");
      const meta = c.meta
        ? `<div class="cl-meta">${c.meta
            .map(([k, v]) => `<span class="cl-chip">${k}: <b>${v}</b></span>`)
            .join("")}</div>`
        : "";
      return `<div class="cl-item${c.open ? " open" : ""}">
      <button type="button" class="cl-head" aria-expanded="${c.open ? "true" : "false"}">
        <span class="v">${c.v}</span>
        <span class="code">${c.code}</span>
        <span class="cl-status ${c.status}">${c.statusLabel}</span>
        <span class="date">${c.date}</span>
        <span class="toggle">${c.open ? "-" : "+"}</span>
      </button>
      <div class="cl-body">
        <p class="cl-summary">${c.summary}</p>
        ${groups}${meta}
      </div>
    </div>`;
    })
    .join("");
}

const PLAN_BADGE: Record<string, string> = {
  next: "Next",
  planned: "Planned",
  milestone: "Milestone",
};

export function releaseLadder(plan: ReleasePlanEntry[]): string {
  return plan
    .map((r) => {
      const kind = r.kind ?? "planned";
      return `<div class="rl k-${kind}">
      <span class="rlstatus">${PLAN_BADGE[kind] ?? "Planned"}</span>
      <span class="rlv">${r.v}</span>
      <span class="rlcode">${r.code}</span>
      <span class="rltheme">${r.theme}</span>
      <ul>${r.items.map((i) => `<li>${i}</li>`).join("")}</ul>
    </div>`;
    })
    .join("");
}

const SPINE_BADGE: Record<ProgramEntry["kind"], string> = {
  shipped: "Shipped",
  next: "Next",
  planned: "Planned",
  milestone: "Milestone",
};

function cardChip(id: string): string {
  return `<button type="button" class="sp-card" data-card="${id}" title="Open card ${id} in the explorer">${id}</button>`;
}

export function programSpine(program: ProgramEntry[]): string {
  let html = "";
  let era: ProgramEntry["era"] | null = null;
  for (const r of program) {
    if (r.era !== era) {
      era = r.era;
      if (era === "pre") html += `<div class="era-mark"><span>The road to 1.0</span></div>`;
      if (era === "post")
        html += `<div class="era-mark"><span>After 1.0, capability waves</span></div>`;
    }
    const note = r.note ? `<span class="sp-note">${r.note}</span>` : "";
    html += `<div class="sp k-${r.kind}">
      <div class="sp-in">
        <div class="sp-top">
          <span class="sp-v">v${r.v}</span>
          <span class="sp-code">${r.code}</span>
          <span class="sp-status">${SPINE_BADGE[r.kind]}</span>
        </div>
        <div class="sp-theme">${r.theme}</div>
        <div class="sp-foot">${r.cards.map(cardChip).join("")}${note}<span class="sp-who">${r.who}</span></div>
      </div>
    </div>`;
  }
  return html;
}

export function dispositionBand(dispositions: Disposition[], active: string | null): string {
  const max = Math.max(...dispositions.map((d) => d.n));
  return dispositions
    .map(
      (d) => `<button type="button" class="db d-${d.key}${active === d.key ? " on" : ""}"
      data-disp="${d.key}" aria-pressed="${active === d.key}">
      <span class="dn">${d.n}</span>
      <span class="dl">${d.label}</span>
      <span class="dbar"><i style="width:${Math.round((d.n / max) * 100)}%"></i></span>
      <span class="dd">${d.blurb}</span>
    </button>`,
    )
    .join("");
}

export function backlogWaves(waves: BacklogWave[]): string {
  return waves
    .map(
      (w) => `<div class="lc">
      <div class="lch"><h4>${w.w}</h4><span class="tag">${w.cards.length} card${w.cards.length === 1 ? "" : "s"}</span></div>
      <p>${w.trig}</p>
      <div class="chip-row">${w.cards.map(cardChip).join("")}</div>
    </div>`,
    )
    .join("");
}

export function personaGrid(personas: Persona[]): string {
  return personas
    .map(
      (p) => `<div class="persona ${p.cls}">
      <span class="idx">${p.idx}</span>
      <h4>${p.name}</h4>
      <div class="role">${p.role}</div>
      <p class="care">${p.care}</p>
      <div class="jr">${p.jr.map((j) => `<span>${j}</span>`).join("")}</div>
    </div>`,
    )
    .join("");
}

export function journeyList(journeys: Journey[]): string {
  return journeys
    .map(
      (j) => `<div class="jrow">
      <button type="button" class="jh" aria-expanded="false">
        <span class="jid">${j.id}</span><span class="jt">${j.t}</span><span class="jp">${j.who}</span><span class="toggle">+</span>
      </button>
      <div class="jb">${j.b}</div>
    </div>`,
    )
    .join("");
}

export function uxContractList(items: string[]): string {
  return items.map((c) => `<li>${c}</li>`).join("");
}

export function coverageMatrix(
  releases: string[],
  coverage: Record<string, number[]>,
  journeys: Journey[],
): string {
  const byId = new Map(journeys.map((j) => [j.id, j]));
  const head = `<tr><th>Journey</th><th></th>${releases
    .map((v) => `<th class="c">v${v}</th>`)
    .join("")}</tr>`;
  const rows = Object.entries(coverage)
    .map(([jid, row]) => {
      const j = byId.get(jid);
      const title = j ? j.t : jid;
      const who = j ? j.who : "";
      return `<tr><td class="jc">${jid}</td><td class="jt">${title}<br><span class="jw">${who}</span></td>${row
        .map((v) => `<td class="c${v ? " hit" : ""}"><i></i></td>`)
        .join("")}</tr>`;
    })
    .join("");
  return `<table class="mx"><thead>${head}</thead><tbody>${rows}</tbody></table>`;
}

export function enablerCards(enablers: Enabler[]): string {
  return enablers
    .map(
      (e) => `<div class="enabler">
      <div class="lab"><span class="glyph">${e.g}</span><div><span class="mono">Enabler ${e.g}</span><span class="ref">${e.ref}</span></div></div>
      <h4>${e.name}</h4>
      <p>${e.p}</p>
      <div class="grades"><span class="g g-enabler">${e.grades}</span>${cardChip(e.id)}</div>
    </div>`,
    )
    .join("");
}

export function shortlistList(items: ShortlistItem[]): string {
  return items.map((s) => `<li><span><b>${s.b}</b> ${s.s}</span></li>`).join("");
}

export function orderingList(items: string[]): string {
  return items
    .map((o, i) => `<li><span class="dot">${i + 1}</span><span>${o}</span></li>`)
    .join("");
}

export function docsList(docs: DocEntry[]): string {
  return docs
    .map((d) => {
      const cls = d.status.toLowerCase().replace(/[^a-z]/g, "");
      return `<div class="doc-row">
      <span class="doc-title">${d.title}</span>
      <span class="doc-status s-${cls}">${d.status}</span>
      <p class="doc-d">${d.d}</p>
    </div>`;
    })
    .join("");
}

export function footerMeta(meta: string[]): string {
  return meta.map((m) => `<span>${m}</span>`).join("");
}
