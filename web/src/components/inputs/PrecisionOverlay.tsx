// The floating precision-decade column shown during a middle-mouse drag
// (UX spec section 7). Pure presentation; the hook owns the numbers.

import { PRECISION_DECADES, ROW_HEIGHT } from "../../hooks/usePrecisionDrag";

interface PrecisionOverlayProps {
  visible: boolean;
  selectedIndex: number;
  position: { x: number; y: number };
}

export function PrecisionOverlay({ visible, selectedIndex, position }: PrecisionOverlayProps) {
  if (!visible) return null;
  const height = PRECISION_DECADES.length * ROW_HEIGHT;
  const width = 80;
  return (
    <div
      className="precision-overlay"
      style={{ left: position.x - width / 2, top: position.y - height / 2 }}
    >
      {PRECISION_DECADES.map((decade, i) => {
        const distance = Math.abs(i - selectedIndex);
        const opacity = distance === 0 ? 1 : distance === 1 ? 0.7 : distance === 2 ? 0.5 : 0.35;
        return (
          <div
            key={decade}
            className={`precision-row${i === selectedIndex ? " selected" : ""}`}
            style={{ opacity }}
          >
            {decade}
          </div>
        );
      })}
    </div>
  );
}
