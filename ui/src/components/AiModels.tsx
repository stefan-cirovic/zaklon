import { useCallback, useEffect, useId, useState } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes } from "../format";
import { usePacks } from "../usePacks";
import { Entries } from "./PackViews";

type T = (k: Key) => string;

/** `fits`: the hub has the memory for it (an older hub does not say). */
type ModelChoice = { id: string; title_en: string; title_sr: string; size: number; installed: boolean; recommended: boolean; fits?: boolean };
/** `recommended` is null when no model fits the hub's memory. */
type AiOverview = { selected: string | null; recommended: string | null; ram_total: number; models: ModelChoice[] };

/**
 * Settings › AI assistant: the one place for AI models. Which downloaded
 * model the assistant uses (the whole household's choice), the one
 * recommended for this computer's memory, and every model with its size, to
 * download, pause or (on the laptop) remove. A model that needs more memory
 * than the computer has says so and cannot be chosen.
 */
export default function AiModels({ t, lang, isHub }: { t: T; lang: Lang; isHub: boolean }) {
  const [ov, setOv] = useState<AiOverview | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const packs = usePacks(t, lang, isHub, { models: true });
  const listId = useId();

  const loadOverview = useCallback(() => {
    api<AiOverview>("/api/assistant")
      .then(setOv)
      .catch((e) => setErr(errText(t, e)));
  }, [t]);
  const models = (packs.data?.packs ?? []).filter((p) => p.category === "model");
  // A model that finished downloading (or was removed) changes what can be chosen.
  const installedKey = models
    .filter((p) => p.state.status === "installed")
    .map((p) => p.id)
    .join(",");
  useEffect(() => {
    loadOverview();
  }, [loadOverview, installedKey]);

  const select = async (id: string) => {
    setBusy(true);
    setErr(null);
    try {
      await api("/api/assistant/model", { json: { id } });
      setOv(await api<AiOverview>("/api/assistant"));
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setBusy(false);
    }
  };

  const title = (m: ModelChoice) => (lang === "sr" ? m.title_sr : m.title_en);
  const installed = ov?.models.filter((m) => m.installed) ?? [];
  const rec = ov?.recommended ? ov.models.find((m) => m.id === ov.recommended) : undefined;
  const selectedTooBig = ov?.models.find((m) => m.id === ov.selected)?.fits === false;
  // No model fits: the assistant is not for this computer, and everything else still works.
  const noAi = !!ov && ov.recommended === null && ov.models.length > 0;
  // The one recommended for this computer's memory is marked, not the catalog's choice.
  const entries = models.map((p) => ({ ...packs.packEntry(p, { removable: true }), recommended: ov?.recommended === p.id }));
  return (
    <div className="stack">
      <div className="panel stack left">
        <h2>{t("aiModelUsed")}</h2>
        {err && <p className="error" role="alert">{err}</p>}
        {ov &&
          (installed.length === 0 ? (
            !noAi && (
              <p style={{ margin: 0 }}>
                {t("aiNeedsModel")}. {t("aiNeedsModelHere")}
              </p>
            )
          ) : (
            <label className="field">
              {t("aiModel")}
              <select value={ov.selected ?? ""} onChange={(e) => select(e.target.value)} disabled={busy}>
                {installed.map((m) => (
                  <option key={m.id} value={m.id} disabled={m.fits === false && m.id !== ov.selected}>
                    {title(m)}
                    {m.recommended ? ` · ${t("recommended")}` : ""}
                    {m.fits === false ? ` · ${t("aiNeedsMoreMemory")}` : ""}
                  </option>
                ))}
              </select>
            </label>
          ))}
        {selectedTooBig && <p className="warn" style={{ margin: 0, fontSize: 14 }}>{t("errAiTooBig")}</p>}
        {ov && rec && (
          <p className="muted" style={{ margin: 0, fontSize: 14 }}>
            {t("aiRecommendedFor")} {fmtBytes(ov.ram_total)} {t("aiRecommendedMemory")}: <strong>{title(rec)}</strong> ({fmtBytes(rec.size)})
          </p>
        )}
        {noAi && (
          <p className="warn" style={{ margin: 0, fontSize: 14 }}>
            {t("aiNotHere")}. {t("aiNotHereLong")}
          </p>
        )}
      </div>
      <section className="stack" aria-labelledby={listId}>
        <h2 id={listId}>{t("catModels")}</h2>
        <p className="muted folder-note">{t("folderModelsDesc")}</p>
        {packs.err && <p className="error" role="alert">{packs.err}</p>}
        {!packs.data && !packs.err && <p className="muted">{t("aiLoading")}</p>}
        {entries.length > 0 && <Entries t={t} entries={entries} view="tiles" glyph="chip" label={t("catModels")} />}
      </section>
    </div>
  );
}
