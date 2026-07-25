// The inline node-type icon: the same 16x16 stroke glyph the canvas chip
// draws (declared key first, category art as the fallback), sized for
// text-adjacent use in the palette, the Add menu, and the list view. Pure
// snapshot interpreter; stroke follows the surrounding text color.

import type { NodeTypeSnapshot } from "../engine/types";
import { glyphPath } from "../flow/nodeVisual";

export function NodeGlyph({
  desc,
  size = 14,
}: {
  desc: NodeTypeSnapshot | undefined;
  size?: number;
}) {
  return (
    <svg
      viewBox="0 0 16 16"
      className="node-glyph-icon"
      width={size}
      height={size}
      aria-hidden
    >
      <path d={glyphPath(desc)} />
    </svg>
  );
}
