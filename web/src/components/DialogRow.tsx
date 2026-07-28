// The shared dialog row and section, used by every settings dialog.
//
// Lifted out of `PreferencesModal`, where `Row` used to live privately, so
// the app has ONE way of laying out a labelled control and ONE way of
// explaining it. Before this there were three idioms in play: bare rows in
// Preferences, an all-in-one paragraph at the bottom of a tab, and inline
// `<small>` help under each checkbox in the web-bundle dialog. Three ways of
// answering the same question is two too many.
//
// The explanation hangs off the LABEL, not the row. Every row contains a
// focusable control, and `Popover` opens on focus as well as hover, so
// wrapping the whole row would pop a tooltip at anyone tabbing through the
// dialog. The label span is not focusable, so hovering it is the only way in.

import type { ReactNode } from "react";
import { Popover, renderDoc } from "./Popover";

/** A titled group of rows.
 *
 * Same shape the shortcuts dialog already used (`.shortcut-group` +
 * `.shortcut-group-title`), generalized rather than reinvented: a flat title
 * with rows under it, no collapsing. A settings dialog you open to change
 * one thing should never make you expand a section to find it.
 */
export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="dialog-section">
      <div className="dialog-section-title">{title}</div>
      {children}
    </div>
  );
}

/** One labelled control, with the explanation on hover.
 *
 * `doc` is required rather than optional, deliberately. An affordance that
 * is present on some rows and not others teaches people to stop reaching
 * for it; a row whose label genuinely explains itself gets a sentence
 * saying what it AFFECTS instead of restating its name.
 */
export function Row({
  label,
  doc,
  children,
}: {
  label: string;
  /** Markdown subset: paragraphs, `code`, **bold** (see `renderDoc`). */
  doc: string;
  children: ReactNode;
}) {
  return (
    <div className="prefs-row">
      <Popover title={label} content={renderDoc(doc)}>
        <span className="prefs-label has-doc">{label}</span>
      </Popover>
      {children}
    </div>
  );
}
