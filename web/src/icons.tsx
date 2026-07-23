// Inline SVG icons: the whole set the chrome needs, no icon-font or
// react-icons dependency. Each renders at 1em so it scales with text.

interface IconProps {
  size?: number;
}

function Svg({ size = 14, children }: IconProps & { children: React.ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export function IconUndo(p: IconProps) {
  // A straight left arrow with a tail (the conventional editor undo).
  return (
    <Svg {...p}>
      <path d="M13.5 8H3" />
      <path d="M7 3.5 2.5 8 7 12.5" />
    </Svg>
  );
}

export function IconRedo(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M2.5 8H13" />
      <path d="M9 3.5 13.5 8 9 12.5" />
    </Svg>
  );
}

export function IconChevronDown(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m4 6 4 4 4-4" />
    </Svg>
  );
}

export function IconChevronRight(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m6 4 4 4-4 4" />
    </Svg>
  );
}

export function IconCheck(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m3.5 8.5 3 3 6-7" />
    </Svg>
  );
}

export function IconClose(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="m4 4 8 8M12 4l-8 8" />
    </Svg>
  );
}

export function IconMaximize(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M6 3H3v3M10 3h3v3M6 13H3v-3M10 13h3v-3" />
    </Svg>
  );
}

export function IconDisplay(p: IconProps) {
  // A monitor: the display-flag motif (wing hover glyph, radial wedge).
  return (
    <Svg {...p}>
      <rect x="2" y="3" width="12" height="8" rx="1.5" />
      <path d="M8 11v2.5M5.5 13.5h5" />
    </Svg>
  );
}

export function IconVisibility(p: IconProps) {
  // The root visibility lamp: a ring with a lit core, echoing the
  // node's vis-dot badge.
  return (
    <Svg {...p}>
      <circle cx="8" cy="8" r="5.5" />
      <circle cx="8" cy="8" r="2.2" fill="currentColor" stroke="none" />
    </Svg>
  );
}

export function IconListView(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M3 4.5h10M3 8h10M3 11.5h10" />
    </Svg>
  );
}

export function IconGraphView(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="2.5" y="2.5" width="5" height="4" rx="1" />
      <rect x="8.5" y="9.5" width="5" height="4" rx="1" />
      <path d="M5 6.5v3.5a1.5 1.5 0 0 0 1.5 1.5h2" />
    </Svg>
  );
}

export function IconBypass(p: IconProps) {
  return (
    <Svg {...p}>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M4.2 11.8 11.8 4.2" />
    </Svg>
  );
}

export function IconTrash(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M3 4.5h10" />
      <path d="M6.5 4.5V3h3v1.5" />
      <path d="M4.5 4.5 5.2 13h5.6l.7-8.5" />
    </Svg>
  );
}

export function IconRename(p: IconProps) {
 // A text cursor between serifs (the radial's rename wedge).
  return (
    <Svg {...p}>
      <path d="M6 3h4M6 13h4M8 3v10" />
      <path d="M11.5 6.5h2v3h-2" />
    </Svg>
  );
}

export function IconDive(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="2.5" y="2.5" width="11" height="11" rx="2" />
      <path d="M8 5.5v5" />
      <path d="M5.8 8.3 8 10.5l2.2-2.2" />
    </Svg>
  );
}

// ---- Viewport transform tools -------------------------------------------
//
// The four live in this registry rather than beside ToolColumn because they
// were the only icons in the codebase defined outside it.
//
// Each glyph is drawn CENTRED ON THE 16x16 GRID: its bounding box is
// symmetric about (8, 8). That is the whole fix for the tools looking
// off-centre in their buttons -- the button already flex-centres the <svg>,
// but centring the box does nothing when the artwork inside it is not
// centred. The old set drew the cursor over x 2..10 and the scale over
// x 1..12.5 in a shared 0 0 14 14 box, so each sat a different distance from
// its button's middle.

export function IconToolSelect(p: IconProps) {
  // An arrow pointer. Solid, so it reads at 19px where a stroked outline
  // would fill in.
  return (
    <Svg {...p}>
      <path
        d="M4.55 2.8 L4.55 12.0 L6.75 9.9 L8.25 13.2 L9.95 12.4 L8.45 9.2 L11.45 8.9 Z"
        fill="currentColor"
        stroke="none"
      />
    </Svg>
  );
}

export function IconToolMove(p: IconProps) {
  // Four-way arrows: symmetric by construction.
  return (
    <Svg {...p}>
      <path d="M8 2.2V13.8M2.2 8h11.6" />
      <path d="M8 2.2 6.1 4.3M8 2.2l1.9 2.1M8 13.8l-1.9-2.1M8 13.8l1.9-2.1" />
      <path d="M2.2 8l2.1-1.9M2.2 8l2.1 1.9M13.8 8l-2.1-1.9M13.8 8l-2.1 1.9" />
    </Svg>
  );
}

export function IconToolRotate(p: IconProps) {
  // An arc around (8, 8) with the arrowhead sitting ON the circle, so the
  // head does not push the bounding box off centre.
  return (
    <Svg {...p}>
      <path d="M11.54 4.46 A5 5 0 1 1 8 3" />
      <path d="M8.1 2.9 L11.9 4.1 L10.6 6.9 Z" fill="currentColor" stroke="none" />
    </Svg>
  );
}

export function IconToolScale(p: IconProps) {
  // Two equal handles on a diagonal: one filled, one open, mirrored about
  // (8, 8).
  return (
    <Svg {...p}>
      <rect x="2.3" y="10.3" width="3.4" height="3.4" fill="currentColor" stroke="none" />
      <rect x="10.3" y="2.3" width="3.4" height="3.4" />
      <path d="M6.4 9.6 9.6 6.4" />
    </Svg>
  );
}

export function IconAttrLabels(p: IconProps) {
  // A value tag beside its anchor point: the labels viz mode.
  return (
    <Svg {...p}>
      <circle cx="3.6" cy="12.4" r="1.4" fill="currentColor" stroke="none" />
      <path d="M6 10.5 8.2 8.3" />
      <rect x="7" y="3" width="7" height="5.2" rx="1" />
      <path d="M8.6 5.6 h3.8" />
    </Svg>
  );
}

export function IconAttrVectors(p: IconProps) {
  // Two arrows leaving their points: the Vec3 arrow viz mode.
  return (
    <Svg {...p}>
      <circle cx="4" cy="12" r="1.3" fill="currentColor" stroke="none" />
      <path d="M5 11 10.4 5.6 M10.4 5.6 h-2.6 M10.4 5.6 v2.6" />
      <circle cx="11.6" cy="12.6" r="1.3" fill="currentColor" stroke="none" />
      <path d="M11.6 11.4 V7 M11.6 7 l-1.6 1.7 M11.6 7 l1.6 1.7" />
    </Svg>
  );
}

export function IconAttrPoints(p: IconProps) {
  // Numbered points: the point-marker viz mode.
  return (
    <Svg {...p}>
      <circle cx="4.4" cy="4.6" r="1.5" fill="currentColor" stroke="none" />
      <circle cx="11.4" cy="6.4" r="1.5" fill="currentColor" stroke="none" />
      <circle cx="6.4" cy="11.6" r="1.5" fill="currentColor" stroke="none" />
      <path d="M12.6 11 h2 M13.6 10 v2" />
    </Svg>
  );
}
