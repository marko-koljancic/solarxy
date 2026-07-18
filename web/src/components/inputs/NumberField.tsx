// The core numeric field: typed entry (Enter commits, Escape reverts,
// blur commits), int snapping, and middle-mouse precision drag with the
// decade overlay. Interaction values flow through the
// caller's preview/commit lanes; while idle the field mirrors `value`.

import { useEffect, useState } from "react";
import { usePrecisionDrag } from "../../hooks/usePrecisionDrag";
import { PrecisionOverlay } from "./PrecisionOverlay";

export interface NumberFieldProps {
  value: number;
  int?: boolean;
  min?: number;
  max?: number;
  step?: number;
  className?: string;
  onPreview: (v: number) => void;
  onCommit: (v: number) => void;
}

function format(v: number): string {
  if (!Number.isFinite(v)) return "0";
  if (Number.isInteger(v)) return String(v);
  // Trim float noise without losing intentional precision.
  return String(Number(v.toFixed(5)));
}

export function NumberField({
  value,
  int = false,
  min,
  max,
  step,
  className,
  onPreview,
  onCommit,
}: NumberFieldProps) {
  const [text, setText] = useState(format(value));
  const [editing, setEditing] = useState(false);

  const clampParse = (raw: string): number | null => {
    const n = Number(raw);
    if (!Number.isFinite(n)) return null;
    let v = int ? Math.round(n) : n;
    if (min !== undefined) v = Math.max(min, v);
    if (max !== undefined) v = Math.min(max, v);
    return v;
  };

  const drag = usePrecisionDrag(value, { min, max, int, onPreview, onCommit });

  // Mirror external changes (undo, preview from the slider, drag) while
  // the user is not typing.
  useEffect(() => {
    if (!editing && !drag.state.dragging) setText(format(value));
  }, [value, editing, drag.state.dragging]);
  useEffect(() => {
    if (drag.state.dragging) setEditing(false);
  }, [drag.state.dragging]);

  const commitText = (raw: string) => {
    const v = clampParse(raw);
    if (v !== null && v !== value) onCommit(v);
    setText(format(v ?? value));
    setEditing(false);
  };

  return (
    <>
      <input
        type="text"
        inputMode="decimal"
        className={`input-field number-field${className ? ` ${className}` : ""}`}
        value={drag.state.dragging ? format(value) : text}
        step={step}
        onFocus={() => setEditing(true)}
        onChange={(e) => {
          setEditing(true);
          setText(e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            commitText((e.target as HTMLInputElement).value);
            (e.target as HTMLInputElement).blur();
          } else if (e.key === "Escape") {
            setText(format(value));
            setEditing(false);
            (e.target as HTMLInputElement).blur();
          }
        }}
        onBlur={(e) => {
          if (editing) commitText(e.target.value);
        }}
        {...drag.bind}
      />
      <PrecisionOverlay
        visible={drag.state.dragging}
        selectedIndex={drag.state.decadeIndex}
        position={drag.state.overlay}
      />
    </>
  );
}
