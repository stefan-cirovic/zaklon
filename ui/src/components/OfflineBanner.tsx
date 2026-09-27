import { useEffect, useState } from "react";
import type { Key } from "../i18n";
import { fmtDateTime } from "../format";
import { offlineState, onOfflineChange } from "../offline";

/** Phones away from the hub: what the screen shows, and what waits to be sent. */
export default function OfflineBanner({ t }: { t: (k: Key) => string }) {
  const [st, setSt] = useState(offlineState);
  useEffect(() => onOfflineChange(() => setSt(offlineState())), []);
  if (st.since === null && st.waiting === 0) return null;
  return (
    <div className="panel notice offline-banner" role="status">
      {st.since !== null && (
        <p style={{ margin: 0 }}>
          <strong>{t("offlineTitle")}</strong> {t("offlineShowing")} {fmtDateTime(new Date(st.since).toISOString())}. {t("offlineShopping")}
        </p>
      )}
      {st.waiting > 0 && (
        <p className="muted" style={{ margin: st.since !== null ? "6px 0 0" : 0, fontSize: 14 }}>
          {t("offlineWaiting")}: {st.waiting}
        </p>
      )}
    </div>
  );
}
