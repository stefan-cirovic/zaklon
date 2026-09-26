import { useEffect, useState } from "react";
import { contentBase } from "../api";
import type { Key, Lang } from "../i18n";
import { cyrToLat, latinArticles } from "../format";

type T = (k: Key) => string;

/** A library article, full screen, with a back button. */
export default function Reader({ t, lang, url, title, onClose }: { t: T; lang: Lang; url: string; title: string; onClose: () => void }) {
  const [base, setBase] = useState<string | null>(null);
  useEffect(() => {
    contentBase().then(setBase).catch(() => setBase(""));
  }, []);
  const latin = latinArticles(lang);
  const shown = latin ? cyrToLat(title) : title;
  const src = latin ? url.replace(/^\/kiwix\//, "/kiwix-lat/") : url;
  return (
    <div className="reader">
      <div className="reader-bar">
        <button className="btn secondary" onClick={onClose}>← {t("back")}</button>
        <div className="reader-title">{shown}</div>
      </div>
      {base !== null && (
        <iframe className="reader-frame" src={base + src} title={shown} sandbox="allow-same-origin allow-popups" referrerPolicy="no-referrer" />
      )}
    </div>
  );
}
