// Pure param-metadata helpers shared by the parameter panel and the
// Properties menu bar: tab derivation, conditional visibility (showIf),
// and the group-to-keys mapping the tab reset dispatches. Kept free of
// React so every rule here is unit-testable.

import type { ParamSnapshot, ParamSource } from "../engine/types";

/** Sentinel tab name for the validation report (registry groups are
 * lowercase, so the capitalized sentinel cannot collide). */
export const VALIDATION_TAB = "Validation";

export function tabLabel(group: string): string {
  if (group === VALIDATION_TAB) return group;
  return group.charAt(0).toUpperCase() + group.slice(1);
}

/** Tab order: general first, the rest in declaration order, plus the
 * Validation tab when a report exists. */
export function paramTabs(params: ParamSnapshot[], hasReport: boolean): string[] {
  const names: string[] = [];
  for (const p of params) {
    if (!names.includes(p.group)) names.push(p.group);
  }
  const ordered = [
    ...names.filter((g) => g.toLowerCase() === "general"),
    ...names.filter((g) => g.toLowerCase() !== "general"),
  ];
  return hasReport ? [...ordered, VALIDATION_TAB] : ordered;
}

/** The stored tab when the node still has it, else the first tab (the
 * fallback that makes switching node types keep a sensible tab). */
export function resolveActiveTab(tabs: string[], stored: string): string | undefined {
  return tabs.includes(stored) ? stored : tabs[0];
}

/** The param keys of one group. The whole group resets regardless of
 * current showIf visibility: a hidden variant row still holds its stored
 * value, and leaving it stale behind a reset would be a surprise later. */
export function groupKeys(params: ParamSnapshot[], group: string): string[] {
  return params.filter((p) => p.group === group).map((p) => p.key);
}

type Params = Record<string, ParamSource | undefined>;

/** The current plain value of `key`: the stored literal's payload, else
 * the declared default (both use the same plain encoding; see the
 * ShowIfPredSnapshot doc on the Rust side). An expression source falls
 * back to the default (v1 refuses to evaluate expressions). */
function currentValue(key: string, specs: ParamSnapshot[], params: Params): unknown {
  const src = params[key];
  if (src && src.kind === "literal") return (src as { value: unknown }).value;
  return specs.find((p) => p.key === key)?.default;
}

function jsonEq(a: unknown, b: unknown): boolean {
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((v, i) => jsonEq(v, b[i]));
  }
  return a === b;
}

/** Evaluates a spec's showIf clauses (ANDed) against the node's current
 * values. A spec with no clauses is always visible. */
export function paramVisible(
  spec: ParamSnapshot,
  specs: ParamSnapshot[],
  params: Params,
): boolean {
  for (const cond of spec.showIf ?? []) {
    const v = currentValue(cond.param, specs, params);
    const p = cond.pred;
    const ok =
      p.kind === "truthy"
        ? Boolean(v)
        : p.kind === "eq"
          ? jsonEq(v, p.value)
          : p.kind === "neq"
            ? !jsonEq(v, p.value)
            : p.values.some((x) => jsonEq(v, x));
    if (!ok) return false;
  }
  return true;
}
