// The turntable speed presets: the single mapping table shared by the
// Display menu's speed submenu and anything else that names a speed. The
// rpm value is the host's global `DisplaySettings.turntable_rpm` (session
// state, never saved into a scene); the persisted default lives in the
// Display preferences.

export const TURNTABLE_SPEEDS: readonly (readonly [label: string, rpm: number])[] = [
  ["Slow (2 rpm)", 2],
  ["Normal (6 rpm)", 6],
  ["Fast (12 rpm)", 12],
  ["Very fast (30 rpm)", 30],
] as const;

/** The preset label for an rpm, or a plain rpm figure for a custom value
 * (a preference can hold any 1..60 rpm). */
export function turntableSpeedLabel(rpm: number): string {
  const preset = TURNTABLE_SPEEDS.find(([, v]) => Math.abs(v - rpm) < 0.01);
  return preset ? preset[0] : `${rpm} rpm`;
}
