import { daysUntil, fmtDate } from "../format";
import type { Key } from "../i18n";

export default function ExpiryBadge({ date, t }: { date: string | null; t: (k: Key) => string }) {
  if (!date) return null;
  const days = daysUntil(date);
  const label = fmtDate(date);
  if (days < 0) return <span className="badge warn">{t("expired")} · {label}</span>;
  if (days <= 30) {
    return (
      <span className="badge soon">
        {days === 0 ? t("today") : `${days} ${t("daysShort")}`} · {label}
      </span>
    );
  }
  return <span className="badge">{label}</span>;
}
