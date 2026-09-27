import { openUrl } from "@tauri-apps/plugin-opener";
import { inTauri } from "../api";

/** The project's website, opened from the logo in the bar. */
export const SITE = "https://zaklon.com";

/**
 * The Zaklon mark (a dome over the horizon with a warm light inside) and the
 * wordmark. Drawn from logo/zaklon-mark.svg; below 32 px the heavier-lined
 * logo/zaklon-mark-small.svg is used so the lines do not vanish.
 */
export function Mark({ size }: { size: number }) {
  const small = size < 32;
  const w = small ? [3, 2.6, 2.4] : [2.2, 1.8, 1.6];
  const star = small ? [1.4, 1.2, 3.8] : [1.0, 0.8, 3.0];
  return (
    <svg className="mark" width={size} height={size} viewBox="0 0 64 64" aria-hidden="true" focusable="false">
      <g transform="translate(32 32) scale(1.14) translate(-32 -36)" strokeLinecap="round">
        <path d="M12 40 A20 20 0 0 1 52 40" fill="none" stroke="#ECE7DD" strokeWidth={w[0]} />
        <line x1="12" y1="40" x2="52" y2="40" stroke="#ECE7DD" strokeWidth={w[0]} />
        <line x1="18" y1="45.5" x2="46" y2="45.5" stroke="#8C8983" strokeWidth={w[1]} />
        <line x1="24" y1="50.5" x2="40" y2="50.5" stroke="#F2B366" strokeWidth={w[2]} />
        <circle cx="13" cy="21" r={star[0]} fill="#8C8983" />
        <circle cx="51" cy="25" r={star[1]} fill="#8C8983" />
        <circle cx="32" cy="31" r={star[2]} fill="#F2B366" />
      </g>
    </svg>
  );
}

/** Mark and wordmark side by side ("row") or stacked ("column"). */
export function Brand({ size, layout = "row", className = "" }: { size: number; layout?: "row" | "column"; className?: string }) {
  return (
    <div className={`brand-lockup ${layout} ${className}`.trim()}>
      <Mark size={size} />
      <span className="wordmark">Zaklon</span>
    </div>
  );
}

/**
 * The logo in the bar: the mark alone at rest; pointing at it (or focusing it)
 * brings the wordmark in below it. It opens the website in the system browser
 * (the desktop and phone apps) or a new tab (a browser).
 */
export function BrandLink({ label }: { label: string }) {
  const open = (e: React.MouseEvent) => {
    if (!inTauri()) return;
    e.preventDefault();
    openUrl(SITE).catch(() => window.open(SITE, "_blank", "noopener"));
  };
  return (
    <a className="nav-brand" href={SITE} target="_blank" rel="noopener" aria-label={label} title={label} onClick={open}>
      <Mark size={34} />
      <span className="wordmark" aria-hidden="true">Zaklon</span>
    </a>
  );
}
