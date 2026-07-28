// Who owns Escape right now.
//
// Escape should always dismiss the INNERMOST thing on screen: a tooltip or
// an open dropdown before the dialog holding it. That is not what the DOM
// gives you for free here, and the reason is worth writing down because it
// bit twice.
//
// `Modal` listens on `window` in the CAPTURE phase, which is the earliest
// point in the whole dispatch. So:
//
//   - A `Popover` listening on `window` capture too cannot win with
//     `stopPropagation`: that stops the event descending, but sibling
//     listeners on the same target still run, and `stopImmediatePropagation`
//     would only help the one that runs FIRST -- always the dialog, since a
//     dialog is mounted long before a tooltip inside it opens.
//   - A `Select` handling Escape in `onKeyDown` (a React bubble handler on
//     the element) is even further behind: capture runs before the target
//     is reached at all.
//
// Both were real: dismissing a tooltip or a theme dropdown inside
// Preferences took the whole dialog down with it, discarding unsaved edits
// to answer "what does this row do?".
//
// So the overlay claims Escape while it is up, and the dialog asks before
// acting. One integer, and no component has to import another's module.

let claims = 0;

/** Called when an overlay above a dialog appears. Pair with `releaseEscape`. */
export function claimEscape(): void {
  claims += 1;
}

export function releaseEscape(): void {
  claims = Math.max(0, claims - 1);
}

/** True while an overlay owns Escape; a dialog underneath must stand down. */
export function isEscapeClaimed(): boolean {
  return claims > 0;
}
