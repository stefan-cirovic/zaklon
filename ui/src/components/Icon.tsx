import type { ReactNode } from "react";

/** Line icons for the bar, the Tools screen and the assistant, drawn on a 24 px grid in the text color. */
export type IconName =
  | "home" | "assistant" | "tools" | "household" | "supplies" | "library" | "maps" | "addons" | "pin"
  | "plus" | "list" | "more" | "close" | "send" | "stop" | "memory" | "shared";

const SHAPES: Record<IconName, ReactNode> = {
  home: (
    <>
      <path d="M3.5 11 12 4l8.5 7" />
      <path d="M5.5 9.5V20H10v-5.5h4V20h4.5V9.5" />
    </>
  ),
  assistant: (
    <>
      <path d="M4 5h16v11H10.5L6 19.5V16H4z" />
      <circle cx="8.5" cy="10.5" r="1" fill="currentColor" stroke="none" />
      <circle cx="12" cy="10.5" r="1" fill="currentColor" stroke="none" />
      <circle cx="15.5" cy="10.5" r="1" fill="currentColor" stroke="none" />
    </>
  ),
  tools: (
    <>
      <rect x="4" y="4" width="6.5" height="6.5" rx="1.5" />
      <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5" />
      <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5" />
      <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5" />
    </>
  ),
  household: (
    <>
      <path d="M4 7h8.5M17.5 7H20M4 17h2.5M11.5 17H20" />
      <circle cx="15" cy="7" r="2.5" />
      <circle cx="9" cy="17" r="2.5" />
    </>
  ),
  supplies: (
    <>
      <path d="M4 7.5 12 4l8 3.5v9L12 20l-8-3.5z" />
      <path d="M4 7.5 12 11l8-3.5M12 11v9" />
    </>
  ),
  library: (
    <>
      <path d="M12 6.5C10.5 5 8 4.5 4 4.5v13c4 0 6.5.5 8 2 1.5-1.5 4-2 8-2v-13c-4 0-6.5.5-8 2z" />
      <path d="M12 6.5v13" />
    </>
  ),
  maps: (
    <>
      <path d="M9 4.5 4 6.5v13l5-2 6 2 5-2v-13l-5 2z" />
      <path d="M9 4.5v13M15 6.5v13" />
    </>
  ),
  addons: (
    <>
      <path d="M12 4v10.5M7.5 10 12 14.5l4.5-4.5" />
      <path d="M4.5 15v4.5h15V15" />
    </>
  ),
  pin: (
    <>
      <path d="M9 4h6l-1 5.5 3 2.5v1.5H7V12l3-2.5z" />
      <path d="M12 13.5V20" />
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" />,
  list: <path d="M4 6.5h16M4 12h16M4 17.5h16" />,
  more: (
    <>
      <circle cx="6" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="18" cy="12" r="1.4" fill="currentColor" stroke="none" />
    </>
  ),
  close: <path d="M6 6l12 12M18 6 6 18" />,
  send: <path d="M12 19V5M6 11l6-6 6 6" />,
  stop: <rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" stroke="none" />,
  memory: (
    <>
      <path d="M7 4h10v16l-5-3.5L7 20z" />
    </>
  ),
  shared: (
    <>
      <path d="M13.5 5H19v5.5M19 5l-8 8" />
      <path d="M17 14v5H5V7h5" />
    </>
  ),
};

export function Icon({ name, size = 22 }: { name: IconName; size?: number }) {
  return <IconFrame size={size}>{SHAPES[name]}</IconFrame>;
}

/** The frame every line icon is drawn in (a 24 px grid, the text color); other icon sets use it too. */
export function IconFrame({ size = 22, children }: { size?: number; children: ReactNode }) {
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
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
