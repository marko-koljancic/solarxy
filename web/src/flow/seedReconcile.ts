// The re-seed reconciliation: what survives when the mirror replaces the
// canvas's node array.
//
// Before this, every graph edit rebuilt each React Flow node from a fresh
// literal, dropping the measurement bookkeeping the library writes back onto
// its node objects after observing them in the DOM. A node dragged before it
// was re-measured had no dimensions, which is the library's error015 ("trying
// to drag a node that is not initialized"), and merely selecting a node or
// typing one character into a parameter field triggered the rebuild.
//
// The mirror stays authoritative: the engine-owned fields (position,
// selection, type, data) always come from the fresh seed, so nothing here
// makes the canvas a second source of truth. What survives is only what the
// canvas itself learned about its own view: the measured size and whatever
// other bookkeeping the library added to the object.
//
// Pure: no DOM, no React, no xyflow import, so it is unit-testable.

/** The fields a seeded node literal carries; everything else on a live node
 * is the canvas library's own bookkeeping and is preserved by identity. */
export interface SeededNode {
  id: string;
  type?: string;
  position: { x: number; y: number };
  selected?: boolean;
  data: { node: unknown; isDisplay: boolean };
}

/** Whether a seeded literal says nothing the live node does not already say.
 * `data.node` compares by reference: the mirror is an immer store, so an
 * untouched node keeps its object identity across events. */
function sameNode<T extends SeededNode>(live: T, seed: T): boolean {
  return (
    live.id === seed.id &&
    live.type === seed.type &&
    live.position.x === seed.position.x &&
    live.position.y === seed.position.y &&
    Boolean(live.selected) === Boolean(seed.selected) &&
    live.data.node === seed.data.node &&
    live.data.isDisplay === seed.data.isDisplay
  );
}

/**
 * Reconcile a fresh seed against the live node array.
 *
 * A node the seed does not change keeps its live object identity, so the
 * canvas re-renders nothing for it. A node the seed does change keeps its
 * live bookkeeping underneath the seeded engine-owned fields, so its
 * measured size survives the edit. An equivalent seed returns the live
 * array itself, so the effect costs no re-render at all.
 *
 * Position is deliberately not preserved: it is engine-owned, arrives on
 * `nodesMoved`, and must overwrite whatever the canvas holds, or undoing a
 * move would not move anything.
 */
export function reconcileNodes<T extends SeededNode>(live: T[], seeded: T[]): T[] {
  let changed = live.length !== seeded.length;
  const byId = new Map(live.map((n) => [n.id, n]));
  const next = seeded.map((seed, i) => {
    const old = byId.get(seed.id);
    if (!old) {
      changed = true;
      return seed;
    }
    if (live[i] !== old) changed = true;
    if (sameNode(old, seed)) return old;
    changed = true;
    return { ...old, ...seed };
  });
  return changed ? next : live;
}
