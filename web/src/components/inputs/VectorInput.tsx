// Vector editor: one labelled NumberField per component (X Y Z W),
// each independently precision-draggable (Minimystix vectorContainer).

import { useState } from "react";
import { NumberField } from "./NumberField";

const COMPONENT_LABELS = ["X", "Y", "Z", "W"];

export interface VectorInputProps {
  value: number[];
  size: 2 | 3 | 4;
  step?: number;
  onPreview: (v: number[]) => void;
  onCommit: (v: number[]) => void;
}

export function VectorInput({ value, size, step, onPreview, onCommit }: VectorInputProps) {
  const [live, setLive] = useState<number[] | null>(null);
  const shown = live ?? value;

  const withComponent = (i: number, v: number): number[] => {
    const next = [...shown];
    while (next.length < size) next.push(0);
    next[i] = v;
    return next.slice(0, size);
  };

  return (
    <div className="vector-row">
      {Array.from({ length: size }, (_, i) => (
        <div key={i} className="vector-component">
          <span className="vector-label">{COMPONENT_LABELS[i]}</span>
          <NumberField
            value={shown[i] ?? 0}
            step={step}
            className="vector-field"
            onPreview={(v) => {
              const next = withComponent(i, v);
              setLive(next);
              onPreview(next);
            }}
            onCommit={(v) => {
              const next = withComponent(i, v);
              setLive(null);
              onCommit(next);
            }}
          />
        </div>
      ))}
    </div>
  );
}
