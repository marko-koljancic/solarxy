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

export function IconEye(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8Z" />
      <circle cx="8" cy="8" r="2" />
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

export function IconDive(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="2.5" y="2.5" width="11" height="11" rx="2" />
      <path d="M8 5.5v5" />
      <path d="M5.8 8.3 8 10.5l2.2-2.2" />
    </Svg>
  );
}
