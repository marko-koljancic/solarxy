// What a render window offers in its pass selector.
//
// The rule about whether a pass can be *shown* is the engine's and is answered
// on the Rust side, once, from the backend's own capability constant. What is
// here is only the presentation of that answer: which rows a selector puts up,
// which of them are choosable, and what an unchoosable one says instead.
//
// Kept out of the component so it can be tested without rendering one, which is
// this frontend's habit for anything that is a rule rather than a widget.

import type { StillPass, StillPasses } from "../engine/types";

/** One row of the selector. */
export interface PassOption {
  value: StillPass;
  label: string;
  /** Why this pass cannot be chosen, or undefined when it can. */
  unavailable?: string;
}

/** The three auxiliary passes, named as the store keys they read. Narrower than
 * `StillPass` on purpose: the beauty is not a key of `StillPasses`, and typing
 * this as the wider union would need an index cast to hide that. */
type AuxPass = Exclude<StillPass, "beauty">;

const AUX: { value: AuxPass; label: string; param: string }[] = [
  { value: "albedo", label: "Albedo", param: "Albedo pass" },
  { value: "normal", label: "Normal", param: "Normal pass" },
  { value: "depth", label: "Depth", param: "Depth pass" },
];

/** The rows a selector shows for this render.
 *
 * A render whose engine writes no auxiliary passes gets the beauty alone rather
 * than three disabled rows for passes no setting could have produced: offering
 * them would suggest a checkbox exists that would help, and none does. */
export function passOptions(passes: StillPasses | undefined): PassOption[] {
  const beauty: PassOption = { value: "beauty", label: "Beauty" };
  if (!passes || !passes.engineWritesAovs) return [beauty];
  return [
    beauty,
    ...AUX.map(({ value, label, param }) => ({
      value,
      label,
      unavailable: passes[value]
        ? undefined
        : `Turn on ${param} on the render node and render again`,
    })),
  ];
}

/** Whether this render can show the pass. The beauty always can. */
export function passAvailable(pass: StillPass, passes: StillPasses | undefined): boolean {
  if (pass === "beauty") return true;
  if (!passes || !passes.engineWritesAovs) return false;
  return passes[pass];
}
