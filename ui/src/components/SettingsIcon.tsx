import type { ReactNode } from "react";
import type { CategoryId } from "../settings";
import { Icon, IconFrame } from "./Icon";

/** Household's line icons: one for each category, and the search, back and "open" marks. */
export type SettingsIconName = Exclude<CategoryId, "assistant"> | "search" | "back" | "chevron";

const dot = (cx: number, cy: number, r = 1) => <circle cx={cx} cy={cy} r={r} fill="currentColor" stroke="none" />;

const SHAPES: Record<SettingsIconName, ReactNode> = {
  devices: (
    <>
      <rect x="7" y="3.5" width="10" height="17" rx="2" />
      <path d="M11 17.5h2" />
    </>
  ),
  network: (
    <>
      <path d="M3.5 9.5a12 12 0 0 1 17 0" />
      <path d="M6.5 12.8a7.7 7.7 0 0 1 11 0" />
      <path d="M9.5 16a3.4 3.4 0 0 1 5 0" />
      {dot(12, 19.2)}
    </>
  ),
  backups: (
    <>
      <path d="M4.6 13.5A7.5 7.5 0 1 0 6.7 6.7" />
      <path d="M4.5 4v3.5H8" />
      <path d="M12 8.5V12l2.5 2" />
    </>
  ),
  privacy: (
    <>
      <path d="M12 3.5 5 6v5.5c0 4.3 2.9 7.7 7 9 4.1-1.3 7-4.7 7-9V6z" />
      <path d="m9 12 2.2 2.2L15.2 10" />
    </>
  ),
  appearance: (
    <>
      <path d="M12 3.5a8.5 8.5 0 1 0 0 17c1.1 0 1.8-.7 1.8-1.6 0-.5-.2-.9-.5-1.2-.3-.4-.5-.8-.5-1.3 0-1 .8-1.8 1.8-1.8h2.1c2.1 0 3.8-1.7 3.8-3.8 0-4.1-3.8-7.3-8.5-7.3z" />
      {dot(7.5, 11.5, 1.1)}
      {dot(9.8, 7.6, 1.1)}
      {dot(14.4, 7.6, 1.1)}
    </>
  ),
  language: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17" />
      <path d="M12 3.5c2.2 2.4 3.3 5.2 3.3 8.5s-1.1 6.1-3.3 8.5c-2.2-2.4-3.3-5.2-3.3-8.5S9.8 5.9 12 3.5z" />
    </>
  ),
  updates: (
    <>
      <path d="M19.5 12a7.5 7.5 0 0 1-13.1 5" />
      <path d="M4.5 12a7.5 7.5 0 0 1 13.1-5" />
      <path d="M17.8 3.6V7h-3.4" />
      <path d="M6.2 20.4V17h3.4" />
    </>
  ),
  about: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5.5" />
      {dot(12, 7.8)}
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6" />
      <path d="m15 15 5 5" />
    </>
  ),
  back: (
    <>
      <path d="M19 12H5.5" />
      <path d="M11 5.5 4.5 12l6.5 6.5" />
    </>
  ),
  chevron: <path d="m9.5 6 6 6-6 6" />,
};

export function SettingsIcon({ name, size = 22 }: { name: SettingsIconName | "assistant"; size?: number }) {
  // The assistant looks the same as in the bar.
  if (name === "assistant") return <Icon name="assistant" size={size} />;
  return <IconFrame size={size}>{SHAPES[name]}</IconFrame>;
}
