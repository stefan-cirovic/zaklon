import type { ReactNode } from "react";

/**
 * Icons of the Add-ons screen, which looks like a file explorer: folders with
 * what is inside drawn on them, drives, and the toolbar's line icons. Drawn on
 * a 24 px grid like components/Icon.tsx. The glyphs of the topics (see
 * topics.ts) mark the same categories on the Tools screen.
 */
export type Glyph = "book" | "health" | "water" | "food" | "garden" | "power" | "build" | "chip" | "map" | "program" | "download";

const GLYPHS: Record<Glyph, ReactNode> = {
  book: (
    <>
      <path d="M12 6.5C10.5 5 8 4.5 4 4.5v13c4 0 6.5.5 8 2 1.5-1.5 4-2 8-2v-13c-4 0-6.5.5-8 2z" />
      <path d="M12 6.5v13" />
    </>
  ),
  health: <path d="M9.5 4.5h5v5h5v5h-5v5h-5v-5h-5v-5h5z" />,
  water: (
    <>
      <path d="M12 3.8c-2.9 3.7-5.6 7-5.6 10.1a5.6 5.6 0 0 0 11.2 0c0-3.1-2.7-6.4-5.6-10.1z" />
      <path d="M9.4 14.2a2.7 2.7 0 0 0 2.4 2.6" />
    </>
  ),
  food: (
    <>
      <path d="M3.5 12h17a8.5 7 0 0 1-17 0z" />
      <path d="M8.5 9c-1-1.2 1-2.3 0-3.5M12 9c-1-1.2 1-2.3 0-3.5M15.5 9c-1-1.2 1-2.3 0-3.5" />
    </>
  ),
  garden: (
    <>
      <path d="M12 20.5v-9" />
      <path d="M12 11.5C12 7.5 9.5 5 5 5c0 4 2.5 6.5 7 6.5z" />
      <path d="M12 14.5c0-3.5 2.5-6 7-6 0 3.5-2.5 6-7 6z" />
    </>
  ),
  power: <path d="M13.5 3.5 5.5 13.5h6l-1 7 8-10h-6z" />,
  build: <path d="M15 4.2a4.3 4.3 0 0 0-4.9 5.9l-5.6 5.6a2 2 0 0 0 2.8 2.8l5.6-5.6A4.3 4.3 0 0 0 18.8 7l-2.6 2.6-2.5-.6-.6-2.5z" />,
  chip: (
    <>
      <rect x="7" y="7" width="10" height="10" rx="1.5" />
      <path d="M10 4v3M14 4v3M10 17v3M14 17v3M4 10h3M4 14h3M17 10h3M17 14h3" />
    </>
  ),
  map: (
    <>
      <path d="M9 4.5 4 6.5v13l5-2 6 2 5-2v-13l-5 2z" />
      <path d="M9 4.5v13M15 6.5v13" />
    </>
  ),
  program: (
    <>
      <rect x="3.5" y="5" width="17" height="14" rx="1.5" />
      <path d="M3.5 9h17M7.5 12.5l2.5 2-2.5 2M12 16.5h4" />
    </>
  ),
  download: (
    <>
      <path d="M12 4v10.5M7.5 10 12 14.5l4.5-4.5" />
      <path d="M4.5 15v4.5h15V15" />
    </>
  ),
};

function Svg({ size, children, className = "icon", viewBox = "0 0 24 24" }: { size: number; children: ReactNode; className?: string; viewBox?: string }) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox={viewBox}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

/** What a pack or folder holds, as a line icon. */
export function GlyphIcon({ glyph, size = 22 }: { glyph: Glyph; size?: number }) {
  return <Svg size={size}>{GLYPHS[glyph]}</Svg>;
}

/** A folder as Explorer draws one: a tab at the back, and on the front what is inside. */
export function FolderIcon({ glyph, size = 48 }: { glyph: Glyph; size?: number }) {
  return (
    <svg className="folder-icon" width={size} height={(size * 40) / 48} viewBox="0 0 48 40" aria-hidden="true" focusable="false">
      <path className="folder-back" d="M2 6a3 3 0 0 1 3-3h12.5l4 4H43a3 3 0 0 1 3 3v4H2z" />
      <rect className="folder-front" x="2" y="11" width="44" height="26" rx="3" />
      <g transform="translate(15 14.5) scale(0.75)" fill="none" stroke="currentColor" strokeWidth={1.9} strokeLinecap="round" strokeLinejoin="round">
        {GLYPHS[glyph]}
      </g>
    </svg>
  );
}

export type DriveKind = "disk" | "usb" | "battery";

/** A drive: a disk, a USB stick, or the laptop's battery (the one device that is not a drive). */
export function DriveIcon({ kind, level = 1, size = 40 }: { kind: DriveKind; level?: number; size?: number }) {
  if (kind === "usb") {
    return (
      <Svg size={size} className="drive-icon">
        <rect x="7" y="9.5" width="10" height="11" rx="2" />
        <path d="M9 9.5v-5h6v5M11 6.8h.01M13 6.8h.01" />
      </Svg>
    );
  }
  if (kind === "battery") {
    const w = Math.max(1, Math.round(12 * Math.min(1, Math.max(0, level))));
    return (
      <Svg size={size} className="drive-icon">
        <rect x="3" y="8" width="16" height="8" rx="2" />
        <path d="M21 11v2" />
        <rect x="5" y="10" width={w} height="4" rx="0.5" fill="currentColor" stroke="none" />
      </Svg>
    );
  }
  return (
    <Svg size={size} className="drive-icon">
      <rect x="3" y="7" width="18" height="10" rx="2" />
      <path d="M3 13h18M6.5 15h.01" />
    </Svg>
  );
}

export type ToolbarIcon = "back" | "tiles" | "details" | "refresh" | "chevron";

const TOOLBAR: Record<ToolbarIcon, ReactNode> = {
  back: <path d="M19 12H5M11 6l-6 6 6 6" />,
  tiles: (
    <>
      <rect x="4" y="4" width="6.5" height="6.5" rx="1" />
      <rect x="13.5" y="4" width="6.5" height="6.5" rx="1" />
      <rect x="4" y="13.5" width="6.5" height="6.5" rx="1" />
      <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1" />
    </>
  ),
  details: <path d="M4 6h16M4 10h16M4 14h16M4 18h16" />,
  refresh: (
    <>
      <path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3" />
      <path d="M19.5 4.5v4h-4" />
    </>
  ),
  chevron: <path d="m9 6 6 6-6 6" />,
};

export function ToolIcon({ name, size = 18 }: { name: ToolbarIcon; size?: number }) {
  return <Svg size={size}>{TOOLBAR[name]}</Svg>;
}
