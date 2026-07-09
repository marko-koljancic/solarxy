// Color editor: swatch + hex text field + hidden native picker
// (Minimystix ColorInput). Values are linear-ish rgba arrays in [0,1];
// alpha passes through untouched.

import { useEffect, useRef, useState } from "react";

export interface ColorInputProps {
  value: number[];
  onCommit: (v: number[]) => void;
}

function toHex(c: number[]): string {
  return `#${[0, 1, 2]
    .map((i) =>
      Math.round(Math.min(1, Math.max(0, c[i] ?? 0)) * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

function parseHex(raw: string): [number, number, number] | null {
  const m = /^#?([0-9a-fA-F]{6})$/.exec(raw.trim());
  if (!m) return null;
  const v = m[1];
  return [
    parseInt(v.slice(0, 2), 16) / 255,
    parseInt(v.slice(2, 4), 16) / 255,
    parseInt(v.slice(4, 6), 16) / 255,
  ];
}

export function ColorInput({ value, onCommit }: ColorInputProps) {
  const rgba = Array.isArray(value) ? value : [0, 0, 0, 1];
  const hex = toHex(rgba);
  const [text, setText] = useState(hex);
  const [editing, setEditing] = useState(false);
  const pickerRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!editing) setText(hex);
  }, [hex, editing]);

  const commitHex = (raw: string) => {
    const rgb = parseHex(raw);
    if (rgb) onCommit([...rgb, rgba[3] ?? 1]);
    setText(rgb ? toHex(rgb) : hex);
    setEditing(false);
  };

  return (
    <div className="color-row">
      <button
        type="button"
        className="color-swatch"
        style={{ background: hex }}
        title="Pick a color"
        onClick={() => pickerRef.current?.click()}
      />
      <input
        type="text"
        className={`input-field color-hex${parseHex(text) === null && editing ? " invalid" : ""}`}
        value={text}
        onFocus={() => setEditing(true)}
        onChange={(e) => {
          setEditing(true);
          setText(e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") commitHex((e.target as HTMLInputElement).value);
          else if (e.key === "Escape") {
            setText(hex);
            setEditing(false);
            (e.target as HTMLInputElement).blur();
          }
        }}
        onBlur={(e) => {
          if (editing) commitHex(e.target.value);
        }}
      />
      <input
        ref={pickerRef}
        type="color"
        className="color-native"
        value={hex}
        onChange={(e) => {
          const rgb = parseHex(e.target.value);
          if (rgb) onCommit([...rgb, rgba[3] ?? 1]);
        }}
      />
    </div>
  );
}
