// Float/int editor: the NumberField plus a range slider when the
// descriptor declares a soft range (Minimystix numberSliderRow layout).
// Slider drags preview per movement and commit exactly once on release;
// a local live value keeps the pair responsive during the drag.

import { useState } from "react";
import { NumberField } from "./NumberField";

export interface FloatInputProps {
  value: number;
  int?: boolean;
  soft?: [number, number] | null;
  min?: number;
  max?: number;
  step?: number;
  onPreview: (v: number) => void;
  onCommit: (v: number) => void;
}

export function FloatInput({ value, int, soft, min, max, step, onPreview, onCommit }: FloatInputProps) {
  const [live, setLive] = useState<number | null>(null);
  const shown = live ?? value;
  const sliderStep = step ?? (int ? 1 : 0.01);

  const snap = (raw: number) => (int ? Math.round(raw) : raw);

  return (
    <div className="number-slider-row">
      {soft && (
        <input
          type="range"
          className="range-slider"
          min={soft[0]}
          max={soft[1]}
          step={sliderStep}
          value={shown}
          onChange={(e) => {
            const v = snap(Number(e.target.value));
            setLive(v);
            onPreview(v);
          }}
          onPointerUp={(e) => {
            const v = snap(Number((e.target as HTMLInputElement).value));
            setLive(null);
            onCommit(v);
          }}
        />
      )}
      <NumberField
        value={shown}
        int={int}
        min={min}
        max={max}
        step={sliderStep}
        onPreview={(v) => {
          setLive(v);
          onPreview(v);
        }}
        onCommit={(v) => {
          setLive(null);
          onCommit(v);
        }}
      />
    </div>
  );
}
