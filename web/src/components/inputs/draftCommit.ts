// When a draft-editing field should actually dispatch.
//
// Every text-like field here (TextField, ExpressionField, NoteNode) holds a
// draft and commits on Enter or blur. Enter does both: it commits and then
// calls blur(), which fires the blur handler. The dispatched value has not
// travelled back through the mirror by then, so a field that compares its
// draft against the STORED prop still sees a difference and dispatches the
// same edit a second time.
//
// The cost is not a wasted round trip, it is a wrong undo stack: the user
// presses undo once, pops the duplicate, and the document does not move.
// Comparing against what was last SENT instead of what is currently stored
// makes the second call a no-op.

/** Whether a draft is a real edit rather than a repeat of the last one. */
export function shouldCommit(draft: string, lastSent: string): boolean {
  return draft !== lastSent;
}
